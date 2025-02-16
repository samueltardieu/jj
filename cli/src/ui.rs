// Copyright 2020 The Jujutsu Authors
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
// https://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

use std::env;
use std::error;
use std::fmt;
use std::io;
use std::io::IsTerminal as _;
use std::io::Stderr;
use std::io::StderrLock;
use std::io::Stdout;
use std::io::StdoutLock;
use std::io::Write;
use std::iter;
use std::mem;
use std::process::Child;
use std::process::ChildStdin;
use std::process::Stdio;
use std::thread;
use std::thread::JoinHandle;

use itertools::Itertools as _;
use jj_lib::config::ConfigGetError;
use jj_lib::config::StackedConfig;
use os_pipe::PipeWriter;
use tracing::instrument;

use crate::command_error::CommandError;
use crate::config::CommandNameAndArgs;
use crate::formatter::Formatter;
use crate::formatter::FormatterFactory;
use crate::formatter::HeadingLabeledWriter;
use crate::formatter::LabeledWriter;
use crate::formatter::PlainTextFormatter;

const BUILTIN_PAGER_NAME: &str = ":builtin";

enum UiOutput {
    Terminal {
        stdout: Stdout,
        stderr: Stderr,
    },
    Paged {
        child: Child,
        child_stdin: ChildStdin,
    },
    BuiltinPaged {
        out_wr: PipeWriter,
        err_wr: PipeWriter,
        pager_thread: JoinHandle<streampager::Result<()>>,
    },
    Null,
}

impl UiOutput {
    fn new_terminal() -> UiOutput {
        UiOutput::Terminal {
            stdout: io::stdout(),
            stderr: io::stderr(),
        }
    }

    fn new_paged(pager_cmd: &CommandNameAndArgs) -> io::Result<UiOutput> {
        let mut cmd = pager_cmd.to_command();
        tracing::info!(?cmd, "spawning pager");
        let mut child = cmd.stdin(Stdio::piped()).spawn()?;
        let child_stdin = child.stdin.take().unwrap();
        Ok(UiOutput::Paged { child, child_stdin })
    }

    fn new_builtin_paged(config: &StreampagerConfig) -> streampager::Result<UiOutput> {
        // This uselessly reads ~/.config/streampager/streampager.toml, even
        // though we then override the important options.
        // TODO(ilyagr): Fix this once a version of streampager with
        // https://github.com/facebook/sapling/pull/1011 is released.
        let mut pager = streampager::Pager::new_using_stdio()?;
        pager.set_wrapping_mode(config.wrapping);
        pager.set_interface_mode(config.streampager_interface_mode());
        // We could make scroll-past-eof configurable, but I'm guessing people
        // will not miss it. If we do make it configurable, we should mention
        // that it's a bad idea to turn this on if `interface=quit-if-one-page`,
        // as it can leave a lot of empty lines on the screen after exiting.
        pager.set_scroll_past_eof(false);

        // Use native pipe, which can be attached to child process. The stdout
        // stream could be an in-process channel, but the cost of extra syscalls
        // wouldn't matter.
        let (out_rd, out_wr) = os_pipe::pipe()?;
        let (err_rd, err_wr) = os_pipe::pipe()?;
        pager.add_stream(out_rd, "")?;
        pager.add_error_stream(err_rd, "stderr")?;

        Ok(UiOutput::BuiltinPaged {
            out_wr,
            err_wr,
            pager_thread: thread::spawn(|| pager.run()),
        })
    }

    fn finalize(self, ui: &Ui) {
        match self {
            UiOutput::Terminal { .. } => { /* no-op */ }
            UiOutput::Paged {
                mut child,
                child_stdin,
            } => {
                drop(child_stdin);
                if let Err(err) = child.wait() {
                    // It's possible (though unlikely) that this write fails, but
                    // this function gets called so late that there's not much we
                    // can do about it.
                    writeln!(
                        ui.warning_default(),
                        "Failed to wait on pager: {err}",
                        err = format_error_with_sources(&err),
                    )
                    .ok();
                }
            }
            UiOutput::BuiltinPaged {
                out_wr,
                err_wr,
                pager_thread,
            } => {
                drop(out_wr);
                drop(err_wr);
                match pager_thread.join() {
                    Ok(Ok(())) => {}
                    Ok(Err(err)) => {
                        writeln!(
                            ui.warning_default(),
                            "Failed to run builtin pager: {err}",
                            err = format_error_with_sources(&err),
                        )
                        .ok();
                    }
                    Err(_) => {
                        writeln!(ui.warning_default(), "Builtin pager crashed.").ok();
                    }
                }
            }
            UiOutput::Null => {}
        }
    }
}

pub enum UiStdout<'a> {
    Terminal(StdoutLock<'static>),
    Paged(&'a ChildStdin),
    Builtin(&'a PipeWriter),
    Null(io::Sink),
}

pub enum UiStderr<'a> {
    Terminal(StderrLock<'static>),
    Paged(&'a ChildStdin),
    Builtin(&'a PipeWriter),
    Null(io::Sink),
}

macro_rules! for_outputs {
    ($ty:ident, $output:expr, $pat:pat => $expr:expr) => {
        match $output {
            $ty::Terminal($pat) => $expr,
            $ty::Paged($pat) => $expr,
            $ty::Builtin($pat) => $expr,
            $ty::Null($pat) => $expr,
        }
    };
}

impl Write for UiStdout<'_> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        for_outputs!(Self, self, w => w.write(buf))
    }

    fn write_all(&mut self, buf: &[u8]) -> io::Result<()> {
        for_outputs!(Self, self, w => w.write_all(buf))
    }

    fn flush(&mut self) -> io::Result<()> {
        for_outputs!(Self, self, w => w.flush())
    }
}

impl Write for UiStderr<'_> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        for_outputs!(Self, self, w => w.write(buf))
    }

    fn write_all(&mut self, buf: &[u8]) -> io::Result<()> {
        for_outputs!(Self, self, w => w.write_all(buf))
    }

    fn flush(&mut self) -> io::Result<()> {
        for_outputs!(Self, self, w => w.flush())
    }
}

pub struct Ui {
    quiet: bool,
    pager: PagerConfig,
    progress_indicator: bool,
    formatter_factory: FormatterFactory,
    output: UiOutput,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Deserialize, clap::ValueEnum)]
#[serde(rename_all = "kebab-case")]
pub enum ColorChoice {
    Always,
    Never,
    Debug,
    Auto,
}

impl fmt::Display for ColorChoice {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            ColorChoice::Always => "always",
            ColorChoice::Never => "never",
            ColorChoice::Debug => "debug",
            ColorChoice::Auto => "auto",
        };
        write!(f, "{s}")
    }
}

fn prepare_formatter_factory(
    config: &StackedConfig,
    stdout: &Stdout,
) -> Result<FormatterFactory, ConfigGetError> {
    let terminal = stdout.is_terminal();
    let (color, debug) = match config.get("ui.color")? {
        ColorChoice::Always => (true, false),
        ColorChoice::Never => (false, false),
        ColorChoice::Debug => (true, true),
        ColorChoice::Auto => (terminal, false),
    };
    if color {
        FormatterFactory::color(config, debug)
    } else if terminal {
        // Sanitize ANSI escape codes if we're printing to a terminal. Doesn't
        // affect ANSI escape codes that originate from the formatter itself.
        Ok(FormatterFactory::sanitized())
    } else {
        Ok(FormatterFactory::plain_text())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Deserialize)]
#[serde(rename_all(deserialize = "kebab-case"))]
pub enum PaginationChoice {
    Never,
    Auto,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Deserialize)]
#[serde(rename_all(deserialize = "kebab-case"))]
pub enum StreampagerAlternateScreenMode {
    QuitIfOnePage,
    FullScreenClearOutput,
    QuitQuicklyOrClearOutput,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Deserialize)]
#[serde(rename_all(deserialize = "kebab-case"))]
enum StreampagerWrappingMode {
    None,
    Word,
    Anywhere,
}

impl From<StreampagerWrappingMode> for streampager::config::WrappingMode {
    fn from(val: StreampagerWrappingMode) -> Self {
        use streampager::config::WrappingMode;
        match val {
            StreampagerWrappingMode::None => WrappingMode::Unwrapped,
            StreampagerWrappingMode::Word => WrappingMode::WordBoundary,
            StreampagerWrappingMode::Anywhere => WrappingMode::GraphemeBoundary,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Deserialize)]
#[serde(rename_all(deserialize = "kebab-case"))]
struct StreampagerConfig {
    interface: StreampagerAlternateScreenMode,
    wrapping: StreampagerWrappingMode,
    // TODO: Add an `quit-quickly-delay-seconds` floating point option or a
    // `quit-quickly-delay` option that takes a 's' or 'ms' suffix. Note that as
    // of this writing, floating point numbers do not work with `--config`
}

impl StreampagerConfig {
    fn streampager_interface_mode(&self) -> streampager::config::InterfaceMode {
        use streampager::config::InterfaceMode;
        use StreampagerAlternateScreenMode::*;
        match self.interface {
            // InterfaceMode::Direct not implemented
            FullScreenClearOutput => InterfaceMode::FullScreen,
            QuitIfOnePage => InterfaceMode::Hybrid,
            QuitQuicklyOrClearOutput => InterfaceMode::Delayed(std::time::Duration::from_secs(2)),
        }
    }
}

enum PagerConfig {
    Disabled,
    Builtin(StreampagerConfig),
    External(CommandNameAndArgs),
}

impl PagerConfig {
    fn from_config(config: &StackedConfig) -> Result<PagerConfig, ConfigGetError> {
        if matches!(config.get("ui.paginate")?, PaginationChoice::Never) {
            return Ok(PagerConfig::Disabled);
        };
        match config.get("ui.pager")? {
            CommandNameAndArgs::String(name) if name == BUILTIN_PAGER_NAME => {
                Ok(PagerConfig::Builtin(config.get("ui.streampager")?))
            }
            pager_command => Ok(PagerConfig::External(pager_command)),
        }
    }
}

impl Ui {
    pub fn null() -> Ui {
        Ui {
            quiet: true,
            pager: PagerConfig::Disabled,
            progress_indicator: false,
            formatter_factory: FormatterFactory::plain_text(),
            output: UiOutput::Null,
        }
    }

    pub fn with_config(config: &StackedConfig) -> Result<Ui, CommandError> {
        let formatter_factory = prepare_formatter_factory(config, &io::stdout())?;
        Ok(Ui {
            quiet: config.get("ui.quiet")?,
            formatter_factory,
            pager: PagerConfig::from_config(config)?,
            progress_indicator: config.get("ui.progress-indicator")?,
            output: UiOutput::new_terminal(),
        })
    }

    pub fn reset(&mut self, config: &StackedConfig) -> Result<(), CommandError> {
        self.quiet = config.get("ui.quiet")?;
        self.pager = PagerConfig::from_config(config)?;
        self.progress_indicator = config.get("ui.progress-indicator")?;
        self.formatter_factory = prepare_formatter_factory(config, &io::stdout())?;
        Ok(())
    }

    /// Switches the output to use the pager, if allowed.
    #[instrument(skip_all)]
    pub fn request_pager(&mut self) {
        if !matches!(&self.output, UiOutput::Terminal { stdout, .. } if stdout.is_terminal()) {
            return;
        }

        let new_output = match &self.pager {
            PagerConfig::Disabled => {
                return;
            }
            PagerConfig::Builtin(streampager_config) => {
                UiOutput::new_builtin_paged(streampager_config)
                    .inspect_err(|err| {
                        writeln!(
                            self.warning_default(),
                            "Failed to set up builtin pager: {err}",
                            err = format_error_with_sources(err),
                        )
                        .ok();
                    })
                    .ok()
            }
            PagerConfig::External(command_name_and_args) => {
                UiOutput::new_paged(command_name_and_args)
                    .inspect_err(|err| {
                        // The pager executable couldn't be found or couldn't be run
                        writeln!(
                            self.warning_default(),
                            "Failed to spawn pager '{name}': {err}",
                            name = command_name_and_args.split_name(),
                            err = format_error_with_sources(err),
                        )
                        .ok();
                        writeln!(self.hint_default(), "Consider using the `:builtin` pager.").ok();
                    })
                    .ok()
            }
        };
        if let Some(output) = new_output {
            self.output = output;
        }
    }

    pub fn color(&self) -> bool {
        self.formatter_factory.is_color()
    }

    pub fn new_formatter<'output, W: Write + 'output>(
        &self,
        output: W,
    ) -> Box<dyn Formatter + 'output> {
        self.formatter_factory.new_formatter(output)
    }

    /// Locked stdout stream.
    pub fn stdout(&self) -> UiStdout<'_> {
        match &self.output {
            UiOutput::Terminal { stdout, .. } => UiStdout::Terminal(stdout.lock()),
            UiOutput::Paged { child_stdin, .. } => UiStdout::Paged(child_stdin),
            UiOutput::BuiltinPaged { out_wr, .. } => UiStdout::Builtin(out_wr),
            UiOutput::Null => UiStdout::Null(io::sink()),
        }
    }

    /// Creates a formatter for the locked stdout stream.
    ///
    /// Labels added to the returned formatter should be removed by caller.
    /// Otherwise the last color would persist.
    pub fn stdout_formatter(&self) -> Box<dyn Formatter + '_> {
        for_outputs!(UiStdout, self.stdout(), w => self.new_formatter(w))
    }

    /// Locked stderr stream.
    pub fn stderr(&self) -> UiStderr<'_> {
        match &self.output {
            UiOutput::Terminal { stderr, .. } => UiStderr::Terminal(stderr.lock()),
            UiOutput::Paged { child_stdin, .. } => UiStderr::Paged(child_stdin),
            UiOutput::BuiltinPaged { err_wr, .. } => UiStderr::Builtin(err_wr),
            UiOutput::Null => UiStderr::Null(io::sink()),
        }
    }

    /// Creates a formatter for the locked stderr stream.
    pub fn stderr_formatter(&self) -> Box<dyn Formatter + '_> {
        for_outputs!(UiStderr, self.stderr(), w => self.new_formatter(w))
    }

    /// Stderr stream to be attached to a child process.
    pub fn stderr_for_child(&self) -> io::Result<Stdio> {
        match &self.output {
            UiOutput::Terminal { .. } => Ok(Stdio::inherit()),
            UiOutput::Paged { child_stdin, .. } => Ok(duplicate_child_stdin(child_stdin)?.into()),
            UiOutput::BuiltinPaged { err_wr, .. } => Ok(err_wr.try_clone()?.into()),
            UiOutput::Null => Ok(Stdio::null()),
        }
    }

    /// Whether continuous feedback should be displayed for long-running
    /// operations
    pub fn use_progress_indicator(&self) -> bool {
        match &self.output {
            UiOutput::Terminal { stderr, .. } => self.progress_indicator && stderr.is_terminal(),
            UiOutput::Paged { .. } => false,
            UiOutput::BuiltinPaged { .. } => false,
            UiOutput::Null => false,
        }
    }

    pub fn progress_output(&self) -> Option<ProgressOutput<std::io::Stderr>> {
        self.use_progress_indicator()
            .then(ProgressOutput::for_stderr)
    }

    /// Writer to print an update that's not part of the command's main output.
    pub fn status(&self) -> Box<dyn Write + '_> {
        if self.quiet {
            Box::new(io::sink())
        } else {
            Box::new(self.stderr())
        }
    }

    /// A formatter to print an update that's not part of the command's main
    /// output. Returns `None` if `--quiet` was requested.
    pub fn status_formatter(&self) -> Option<Box<dyn Formatter + '_>> {
        (!self.quiet).then(|| self.stderr_formatter())
    }

    /// Writer to print hint with the default "Hint: " heading.
    pub fn hint_default(
        &self,
    ) -> HeadingLabeledWriter<Box<dyn Formatter + '_>, &'static str, &'static str> {
        self.hint_with_heading("Hint: ")
    }

    /// Writer to print hint without the "Hint: " heading.
    pub fn hint_no_heading(&self) -> LabeledWriter<Box<dyn Formatter + '_>, &'static str> {
        let formatter = self
            .status_formatter()
            .unwrap_or_else(|| Box::new(PlainTextFormatter::new(io::sink())));
        LabeledWriter::new(formatter, "hint")
    }

    /// Writer to print hint with the given heading.
    pub fn hint_with_heading<H: fmt::Display>(
        &self,
        heading: H,
    ) -> HeadingLabeledWriter<Box<dyn Formatter + '_>, &'static str, H> {
        self.hint_no_heading().with_heading(heading)
    }

    /// Writer to print warning with the default "Warning: " heading.
    pub fn warning_default(
        &self,
    ) -> HeadingLabeledWriter<Box<dyn Formatter + '_>, &'static str, &'static str> {
        self.warning_with_heading("Warning: ")
    }

    /// Writer to print warning without the "Warning: " heading.
    pub fn warning_no_heading(&self) -> LabeledWriter<Box<dyn Formatter + '_>, &'static str> {
        LabeledWriter::new(self.stderr_formatter(), "warning")
    }

    /// Writer to print warning with the given heading.
    pub fn warning_with_heading<H: fmt::Display>(
        &self,
        heading: H,
    ) -> HeadingLabeledWriter<Box<dyn Formatter + '_>, &'static str, H> {
        self.warning_no_heading().with_heading(heading)
    }

    /// Writer to print error without the "Error: " heading.
    pub fn error_no_heading(&self) -> LabeledWriter<Box<dyn Formatter + '_>, &'static str> {
        LabeledWriter::new(self.stderr_formatter(), "error")
    }

    /// Writer to print error with the given heading.
    pub fn error_with_heading<H: fmt::Display>(
        &self,
        heading: H,
    ) -> HeadingLabeledWriter<Box<dyn Formatter + '_>, &'static str, H> {
        self.error_no_heading().with_heading(heading)
    }

    /// Waits for the pager exits.
    #[instrument(skip_all)]
    pub fn finalize_pager(&mut self) {
        let old_output = mem::replace(&mut self.output, UiOutput::new_terminal());
        old_output.finalize(self);
    }

    pub fn can_prompt() -> bool {
        io::stderr().is_terminal()
            || env::var("JJ_INTERACTIVE")
                .map(|v| v == "1")
                .unwrap_or(false)
    }

    pub fn prompt(&self, prompt: &str) -> io::Result<String> {
        if !Self::can_prompt() {
            return Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "Cannot prompt for input since the output is not connected to a terminal",
            ));
        }
        write!(self.stderr(), "{prompt}: ")?;
        self.stderr().flush()?;
        let mut buf = String::new();
        io::stdin().read_line(&mut buf)?;

        if buf.is_empty() {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "Prompt cancelled by EOF",
            ));
        }

        if let Some(trimmed) = buf.strip_suffix('\n') {
            buf.truncate(trimmed.len());
        }
        Ok(buf)
    }

    /// Repeat the given prompt until the input is one of the specified choices.
    pub fn prompt_choice(
        &self,
        prompt: &str,
        choices: &[impl AsRef<str>],
        default: Option<&str>,
    ) -> io::Result<String> {
        if !Self::can_prompt() {
            if let Some(default) = default {
                // Choose the default automatically without waiting.
                writeln!(self.stderr(), "{prompt}: {default}")?;
                return Ok(default.to_owned());
            }
        }

        loop {
            let choice = self.prompt(prompt)?.trim().to_owned();
            if choice.is_empty() {
                if let Some(default) = default {
                    return Ok(default.to_owned());
                }
            }
            if choices.iter().any(|c| choice == c.as_ref()) {
                return Ok(choice);
            }

            writeln!(self.warning_no_heading(), "unrecognized response")?;
        }
    }

    /// Prompts for a yes-or-no response, with yes = true and no = false.
    pub fn prompt_yes_no(&self, prompt: &str, default: Option<bool>) -> io::Result<bool> {
        let default_str = match &default {
            Some(true) => "(Yn)",
            Some(false) => "(yN)",
            None => "(yn)",
        };
        let default_choice = default.map(|c| if c { "Y" } else { "N" });

        let choice = self.prompt_choice(
            &format!("{prompt} {default_str}"),
            &["y", "n", "yes", "no", "Yes", "No", "YES", "NO"],
            default_choice,
        )?;
        Ok(choice.starts_with(['y', 'Y']))
    }

    pub fn prompt_password(&self, prompt: &str) -> io::Result<String> {
        if !io::stdout().is_terminal() {
            return Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "Cannot prompt for input since the output is not connected to a terminal",
            ));
        }
        rpassword::prompt_password(format!("{prompt}: "))
    }

    pub fn term_width(&self) -> usize {
        term_width().unwrap_or(80).into()
    }
}

#[derive(Debug)]
pub struct ProgressOutput<W> {
    output: W,
    term_width: Option<u16>,
}

impl ProgressOutput<io::Stderr> {
    pub fn for_stderr() -> ProgressOutput<io::Stderr> {
        ProgressOutput {
            output: io::stderr(),
            term_width: None,
        }
    }
}

impl<W> ProgressOutput<W> {
    pub fn for_test(output: W, term_width: u16) -> Self {
        Self {
            output,
            term_width: Some(term_width),
        }
    }

    pub fn term_width(&self) -> Option<u16> {
        // Terminal can be resized while progress is displayed, so don't cache it.
        self.term_width.or_else(term_width)
    }

    /// Construct a guard object which writes `text` when dropped. Useful for
    /// restoring terminal state.
    pub fn output_guard(&self, text: String) -> OutputGuard {
        OutputGuard {
            text,
            output: io::stderr(),
        }
    }
}

impl<W: Write> ProgressOutput<W> {
    pub fn write_fmt(&mut self, fmt: fmt::Arguments<'_>) -> io::Result<()> {
        self.output.write_fmt(fmt)
    }

    pub fn flush(&mut self) -> io::Result<()> {
        self.output.flush()
    }
}

pub struct OutputGuard {
    text: String,
    output: Stderr,
}

impl Drop for OutputGuard {
    #[instrument(skip_all)]
    fn drop(&mut self) {
        _ = self.output.write_all(self.text.as_bytes());
        _ = self.output.flush();
    }
}

#[cfg(unix)]
fn duplicate_child_stdin(stdin: &ChildStdin) -> io::Result<std::os::fd::OwnedFd> {
    use std::os::fd::AsFd as _;
    stdin.as_fd().try_clone_to_owned()
}

#[cfg(windows)]
fn duplicate_child_stdin(stdin: &ChildStdin) -> io::Result<std::os::windows::io::OwnedHandle> {
    use std::os::windows::io::AsHandle as _;
    stdin.as_handle().try_clone_to_owned()
}

fn format_error_with_sources(err: &dyn error::Error) -> impl fmt::Display + '_ {
    iter::successors(Some(err), |&err| err.source()).format(": ")
}

fn term_width() -> Option<u16> {
    if let Some(cols) = env::var("COLUMNS").ok().and_then(|s| s.parse().ok()) {
        Some(cols)
    } else {
        crossterm::terminal::size().ok().map(|(cols, _)| cols)
    }
}
