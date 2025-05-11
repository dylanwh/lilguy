use eyre::Result;
use indexmap::IndexMap;
use mlua::prelude::*;
use nu_ansi_term::{Color, Style};
use parking_lot::Mutex;
use reedline::{
    DefaultHinter, ExternalPrinter, FileBackedHistory, Highlighter, Prompt, PromptEditMode,
    PromptViMode, Reedline, Signal, StyledText, Validator,
};
use serde::{Deserialize, Serialize};
use std::{borrow::Cow, path::PathBuf, sync::Arc};
use tokio::sync::mpsc::{Receiver, Sender};
use tokio_util::{sync::CancellationToken, task::TaskTracker};

use crate::{runtime, Output};

pub type LuaHighlighterConfig = IndexMap<String, LuaStyle>;

pub async fn start(
    token: &CancellationToken,
    tracker: &TaskTracker,
    config: &crate::command::Config,
    output: &Output,
    lua: Lua,
) -> Result<(), eyre::Report> {
    let config = config.shell.clone();
    let highlighter = LuaHighlighter::new(config.highlighter)?;
    let history_file = config
        .history
        .file
        .clone()
        .or_else(|| {
            let data_dir = dirs::data_dir()?;
            Some(data_dir.join(env!("CARGO_PKG_NAME")).join("history.txt"))
        })
        .expect("could not determine history file");
    let history_size = config.history.size.unwrap_or(1000);
    let hinter_style = &config.hinter.style;
    let prompt_config = config.prompt;
    tokio::fs::create_dir_all(history_file.parent().expect("history file has no parent"))
        .await
        .expect("could not create history file directory");
    let printer = ExternalPrinter::default();
    output.set_printer(printer.clone());

    // replace lua print function with our own
    let globals = lua.globals();
    let lua_printer = printer.clone();
    let print = lua.create_function(move |_lua, args: LuaMultiValue| {
        let mut line = String::new();
        for arg in args {
            if !line.is_empty() {
                line.push('\t');
            }
            line.push_str(&arg.to_string()?);
        }
        lua_printer.print(line).into_lua_err()?;
        Ok(())
    })?;
    globals.set("print", print)?;

    let reedline = Reedline::create()
        .with_validator(Box::new(LuaValidator {
            parser: Mutex::new(new_lua_parser()),
        }))
        .with_highlighter(Box::new(highlighter.clone()))
        .with_hinter(Box::new(
            DefaultHinter::default().with_style(hinter_style.into()),
        ))
        .with_external_printer(printer.clone())
        .with_history(Box::new(FileBackedHistory::with_file(
            history_size,
            history_file,
        )?));
    let (tx, rx) = tokio::sync::mpsc::channel(1);

    tracker.spawn_blocking(move || read_loop(reedline, prompt_config, tx));
    tracker.spawn(eval_loop(token.clone(), rx, printer, highlighter, lua));

    Ok(())
}

async fn eval_loop(
    token: CancellationToken,
    mut rx: Receiver<String>,
    printer: ExternalPrinter<String>,
    highlighter: LuaHighlighter,
    lua: Lua,
) {
    tracing::info!("starting eval loop");
    while let Some(input) = read_line(&token, &mut rx).await {
        match lua.load(&input).eval_async().await {
            Ok(results) => {
                for expr in runtime::dump::to_strings(results) {
                    let code = highlighter.highlight(&expr, 0);
                    printer
                        .print(code.render_simple())
                        .expect("could not print result");
                }
            }
            Err(e) => {
                printer.print(format!("error: {}", e)).unwrap();
            }
        }
    }
    token.cancel();
    tracing::info!("exiting eval loop");
}

async fn read_line<R>(token: &CancellationToken, rx: &mut Receiver<R>) -> Option<R> {
    tokio::select! {
        _ = token.cancelled() => None,
        line = rx.recv() => line,
    }
}

fn read_loop(
    mut reedline: Reedline,
    prompt_config: PromptConfig,
    tx: Sender<String>,
) -> Result<()> {
    loop {
        match reedline.read_line(&prompt_config) {
            Ok(Signal::Success(input)) => {
                if tx.blocking_send(input).is_err() {
                    break;
                }
            }
            Ok(Signal::CtrlC) => {
                println!("^C");
            }
            Ok(Signal::CtrlD) => {
                tracing::info!("^D");
                break;
            }
            Err(e) => {
                eprintln!("Error: {}", e);
                break;
            }
        }
    }

    Ok(())
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub highlighter: LuaHighlighterConfig,
    pub hinter: HinterConfig,
    pub prompt: PromptConfig,
    pub history: HistoryConfig,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            highlighter: LuaHighlighterConfig::from([
                // Keywords and control flow
                (
                    "keyword".to_string(),
                    LuaStyle {
                        foreground: Some(Color::Purple),
                        ..Default::default()
                    },
                ),
                // Functions and calls
                (
                    "function".to_string(),
                    LuaStyle {
                        foreground: Some(Color::Yellow),
                        ..Default::default()
                    },
                ),
                (
                    "function.call".to_string(),
                    LuaStyle {
                        foreground: Some(Color::Yellow),
                        ..Default::default()
                    },
                ),
                (
                    "method".to_string(),
                    LuaStyle {
                        foreground: Some(Color::LightYellow),
                        ..Default::default()
                    },
                ),
                (
                    "method.call".to_string(),
                    LuaStyle {
                        foreground: Some(Color::LightYellow),
                        ..Default::default()
                    },
                ),
                // Variables and parameters
                (
                    "variable".to_string(),
                    LuaStyle {
                        ..Default::default()
                    },
                ),
                (
                    "variable.builtin".to_string(),
                    LuaStyle {
                        foreground: Some(Color::Rgb(0, 255, 255)),
                        is_bold: true,
                        ..Default::default()
                    },
                ),
                (
                    "parameter".to_string(),
                    LuaStyle {
                        foreground: Some(Color::LightCyan),
                        ..Default::default()
                    },
                ),
                // Literals
                (
                    "string".to_string(),
                    LuaStyle {
                        foreground: Some(Color::LightRed),
                        ..Default::default()
                    },
                ),
                (
                    "number".to_string(),
                    LuaStyle {
                        foreground: Some(Color::LightGreen),
                        ..Default::default()
                    },
                ),
                (
                    "boolean".to_string(),
                    LuaStyle {
                        foreground: Some(Color::LightBlue),
                        ..Default::default()
                    },
                ),
                (
                    "nil".to_string(),
                    LuaStyle {
                        foreground: Some(Color::LightBlue),
                        ..Default::default()
                    },
                ),
                // Operators and punctuation
                (
                    "operator".to_string(),
                    LuaStyle {
                        ..Default::default()
                    },
                ),
                (
                    "punctuation.delimiter".to_string(),
                    LuaStyle {
                        ..Default::default()
                    },
                ),
                (
                    "punctuation.bracket".to_string(),
                    LuaStyle {
                        ..Default::default()
                    },
                ),
                // Comments
                (
                    "comment".to_string(),
                    LuaStyle {
                        foreground: Some(Color::White),
                        ..Default::default()
                    },
                ),
                // Tables
                (
                    "table".to_string(),
                    LuaStyle {
                        foreground: Some(Color::Yellow),
                        ..Default::default()
                    },
                ),
                (
                    "field".to_string(),
                    LuaStyle {
                        foreground: Some(Color::LightBlue),
                        ..Default::default()
                    },
                ),
                // Error handling
                (
                    "error".to_string(),
                    LuaStyle {
                        foreground: Some(Color::Red),
                        ..Default::default()
                    },
                ),
            ]),
            hinter: HinterConfig {
                style: LuaStyle {
                    foreground: Some(Color::DarkGray),
                    ..Default::default()
                },
            },
            prompt: PromptConfig::default(),
            history: HistoryConfig::default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PromptConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub left: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub right: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub indicator: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub multiline_indicator: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub history_search_indicator: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct HistoryConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size: Option<usize>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file: Option<PathBuf>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct HinterConfig {
    #[serde(default)]
    pub style: LuaStyle,
}

struct LuaValidator {
    parser: Mutex<tree_sitter::Parser>,
}

impl Validator for LuaValidator {
    fn validate(&self, line: &str) -> reedline::ValidationResult {
        let tree = self
            .parser
            .lock()
            .parse(line, None)
            .expect("Failed to parse");

        let has_error = tree.root_node().has_error();

        if has_error {
            let tree = self
                .parser
                .lock()
                .parse(format!("return {line}"), None)
                .expect("Failed to parse");
            let has_error = tree.root_node().has_error();
            if has_error {
                reedline::ValidationResult::Incomplete
            } else {
                reedline::ValidationResult::Complete
            }
        } else {
            reedline::ValidationResult::Complete
        }
    }
}

#[derive(Clone)]
struct LuaHighlighter {
    inner: Arc<LuaHighlighterInner>,
}

struct LuaHighlighterInner {
    highlighter: Mutex<tree_sitter_highlight::Highlighter>,
    config: tree_sitter_highlight::HighlightConfiguration,
    theme: LuaHighlighterConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct LuaStyle {
    #[serde(alias = "fg", default, skip_serializing_if = "Option::is_none")]
    foreground: Option<Color>,

    #[serde(alias = "bg", default, skip_serializing_if = "Option::is_none")]
    background: Option<Color>,

    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    is_bold: bool,

    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    is_dimmed: bool,

    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    is_italic: bool,

    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    is_underline: bool,

    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    is_blink: bool,

    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    is_reverse: bool,

    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    is_hidden: bool,

    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    is_strikethrough: bool,

    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    prefix_with_reset: bool,
}

impl From<&LuaStyle> for Style {
    fn from(style: &LuaStyle) -> Self {
        let LuaStyle {
            foreground,
            background,
            is_bold,
            is_dimmed,
            is_italic,
            is_underline,
            is_blink,
            is_reverse,
            is_hidden,
            is_strikethrough,
            prefix_with_reset,
        } = style;
        Self {
            foreground: *foreground,
            background: *background,
            is_bold: *is_bold,
            is_dimmed: *is_dimmed,
            is_italic: *is_italic,
            is_underline: *is_underline,
            is_blink: *is_blink,
            is_reverse: *is_reverse,
            is_hidden: *is_hidden,
            is_strikethrough: *is_strikethrough,
            prefix_with_reset: *prefix_with_reset,
        }
    }
}

impl LuaHighlighter {
    fn new(theme: LuaHighlighterConfig) -> Result<Self> {
        let lua_language = tree_sitter_lua::LANGUAGE.into();
        let highlighter = tree_sitter_highlight::Highlighter::new();
        let mut config = tree_sitter_highlight::HighlightConfiguration::new(
            lua_language,
            "lua",
            tree_sitter_lua::HIGHLIGHTS_QUERY,
            tree_sitter_lua::INJECTIONS_QUERY,
            tree_sitter_lua::LOCALS_QUERY,
        )?;
        let names = theme.keys().collect::<Vec<&String>>();
        config.configure(&names);

        Ok(Self {
            inner: Arc::new(LuaHighlighterInner {
                highlighter: Mutex::new(highlighter),
                config,
                theme,
            }),
        })
    }
}

impl reedline::Highlighter for LuaHighlighter {
    fn highlight(&self, line: &str, _cursor: usize) -> reedline::StyledText {
        let mut highlighter = self.inner.highlighter.lock();

        let highlights = highlighter
            .highlight(&self.inner.config, line.as_bytes(), None, |_| None)
            .expect("highlighter should return highlights");

        let mut style = None;
        let mut styled_text = StyledText::new();
        for event in highlights {
            match event.unwrap() {
                tree_sitter_highlight::HighlightEvent::HighlightStart(s) => {
                    style = self.inner.theme.get_index(s.0).map(|(_, v)| v);
                }
                tree_sitter_highlight::HighlightEvent::Source { start, end } => {
                    let style = style.map_or_else(Style::new, std::convert::Into::into);
                    styled_text.push((style, line[start..end].to_owned()));
                }
                tree_sitter_highlight::HighlightEvent::HighlightEnd => {
                    style = None;
                }
            }
        }

        styled_text
    }
}

impl Prompt for PromptConfig {
    fn render_prompt_left(&self) -> Cow<str> {
        self.left.as_deref().unwrap_or(">>> ").into()
    }

    fn render_prompt_right(&self) -> Cow<str> {
        self.right.as_deref().unwrap_or("").into()
    }

    fn render_prompt_indicator(&self, arg: PromptEditMode) -> Cow<str> {
        let mode = match arg {
            PromptEditMode::Default | PromptEditMode::Emacs => "",
            PromptEditMode::Vi(ref prompt_vi_mode) => match prompt_vi_mode {
                PromptViMode::Normal => "[normal] ",
                PromptViMode::Insert => "[insert] ",
            },
            PromptEditMode::Custom(ref s) => s,
        };
        self.indicator
            .as_deref()
            .unwrap_or("{mode}")
            .replace("{mode}", mode)
            .into()
    }

    fn render_prompt_multiline_indicator(&self) -> Cow<str> {
        self.multiline_indicator.as_deref().unwrap_or("... ").into()
    }

    fn render_prompt_history_search_indicator(
        &self,
        history_search: reedline::PromptHistorySearch,
    ) -> Cow<str> {
        let status = match history_search.status {
            reedline::PromptHistorySearchStatus::Passing => "passing",
            reedline::PromptHistorySearchStatus::Failing => "failing",
        };
        let term = history_search.term;
        self.history_search_indicator
            .as_deref()
            .unwrap_or("(reverse-i-search)`{term}': {status} ")
            .replace("{status}", status)
            .replace("{term}", &term)
            .into()
    }
}

fn new_lua_parser() -> tree_sitter::Parser {
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&tree_sitter_lua::LANGUAGE.into())
        .expect("Error loading Lua grammar");
    parser
}
