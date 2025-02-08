extern crate rustyline;
extern crate rustyline_derive;
extern crate termion;

use std::borrow::Cow;
use std::collections::HashMap;
use std::env;
use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io::{self, BufRead as _, BufReader, BufWriter, Read, Write};
use std::iter::Peekable;
use std::mem;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::str::Chars;

use rustyline::error::ReadlineError;
use rustyline::highlight::Highlighter;
use rustyline::hint::Hinter;
use rustyline::{Behavior, Config, Context, Editor};
use rustyline_derive::{Completer, Helper, Validator};

#[derive(Debug)]
struct Entry {
    term: String,
    phrases: Vec<Phrase>,
}

impl Entry {
    fn parse(mut input: Peekable<Chars>) -> Option<Entry> {
        match input.peek() {
            Some(';') | None => None,
            Some(_) => {
                let mut term = String::new();
                while let Some(c) = input.next() {
                    match c {
                        ' ' if input.peek() == Some(&'/') => {
                            input.next(); // skip '/'
                            break;
                        }
                        _ => term.push(c),
                    }
                }
                let mut phrases = Vec::new();
                let mut body = String::new();
                let mut comment = String::new();
                let mut is_comment = false;
                while let Some(c) = input.next() {
                    match c {
                        '/' => {
                            let phrase = Phrase {
                                body: mem::take(&mut body),
                                comment: mem::take(&mut comment),
                            };
                            phrases.push(phrase);
                            is_comment = false;
                        }
                        ';' => {
                            is_comment = true;
                        }
                        _ => {
                            if is_comment {
                                comment.push(c);
                            } else {
                                body.push(c);
                            }
                        }
                    }
                }
                Some(Entry { term, phrases })
            }
        }
    }
}

#[derive(Debug)]
struct Question {
    index: usize,
    entry: Rc<Entry>,
}

#[derive(Debug, Completer, Helper, Validator)]
struct QuestionHint {
    entry: Rc<Entry>,
    mistakes: usize,
}

impl Hinter for QuestionHint {
    type Hint = String;

    fn hint(&self, line: &str, _pos: usize, _ctx: &Context<'_>) -> Option<Self::Hint> {
        let mut symbols = 0;
        let hint_string = self
            .entry
            .term
            .chars()
            .enumerate()
            .map(|(i, c)| {
                if !c.is_ascii_alphabetic() {
                    symbols += 1;
                    c
                } else if i - symbols < self.mistakes {
                    c
                } else {
                    '_'
                }
            })
            .skip(line.chars().count())
            .collect();
        Some(hint_string)
    }
}

impl Highlighter for QuestionHint {
    fn highlight_hint<'h>(&self, hint: &'h str) -> Cow<'h, str> {
        Cow::Owned(format!(
            "{}{}{}",
            termion::color::Fg(termion::color::LightBlack),
            hint,
            termion::style::Reset,
        ))
    }
}

#[derive(Debug)]
struct Phrase {
    body: String,
    comment: String,
}

struct GameUI {
    readline: Editor<QuestionHint>,
}

impl GameUI {
    fn new() -> Self {
        let config = Config::builder().behavior(Behavior::PreferTerm).build();
        let readline = Editor::<QuestionHint>::with_config(config);
        Self { readline }
    }

    fn notify_question(&mut self, question: &Question, _state: &GameState) {
        print!(
            "{}{}Q{}{} ",
            termion::style::Bold,
            termion::color::Fg(termion::color::LightYellow),
            question.index + 1,
            termion::style::Reset,
        );
        for phrase in question.entry.phrases.iter() {
            if phrase.comment.is_empty() {
                print!(
                    "/{}{}{}{}",
                    termion::style::Bold,
                    termion::color::Fg(termion::color::LightBlue),
                    phrase.body,
                    termion::style::Reset,
                );
            } else {
                print!(
                    "/{}{}{}{};{}{}",
                    termion::style::Bold,
                    termion::color::Fg(termion::color::LightBlue),
                    phrase.body,
                    termion::color::Fg(termion::color::LightBlack),
                    phrase.comment,
                    termion::style::Reset,
                );
            }
        }
        println!("/");
    }

    fn notify_correct(&mut self, question: &Question, state: &GameState) {
        let score = state.get_score(&question.entry.term).unwrap_or_default();
        if state.mistakes == 0 {
            println!(
                "{}{}> {} {}(perfect, {} try, {}% correct){}",
                termion::cursor::Up(1),
                termion::clear::CurrentLine,
                question.entry.term,
                termion::color::Fg(termion::color::LightGreen),
                OrdinalNum(score.total_tries()),
                (score.correct_rate() * 100.0).round(),
                termion::style::Reset,
            );
        } else {
            println!(
                "{}{}> {} {}({} mistakes, {} try, {}% correct){}",
                termion::cursor::Up(1),
                termion::clear::CurrentLine,
                question.entry.term,
                termion::color::Fg(termion::color::LightRed),
                state.mistakes,
                OrdinalNum(score.total_tries()),
                (score.correct_rate() * 100.0).round(),
                termion::style::Reset,
            );
        }
    }

    fn notify_incorrect(&mut self, _question: &Question, _state: &GameState) {
        println!(
            "{}{}{}",
            termion::cursor::Up(1),
            termion::clear::CurrentLine,
            termion::cursor::Up(1),
        );
    }

    fn wait_for_input(&mut self, hint: QuestionHint) -> Result<UIResponse, ReadlineError> {
        self.readline.set_helper(Some(hint));
        match self.readline.readline("> ") {
            Ok(input) if input.starts_with(":") => {
                let command = input.get(1..).unwrap_or_default();
                if "quit".starts_with(command) {
                    Ok(UIResponse::Quit)
                } else {
                    Ok(UIResponse::Return(input))
                }
            }
            Ok(input) => Ok(UIResponse::Return(input)),
            Err(ReadlineError::Interrupted | ReadlineError::Eof) => Ok(UIResponse::Quit),
            Err(error) => Err(error),
        }
    }
}

type Scores = HashMap<String, Score>;

#[derive(Clone, Debug, Default)]
struct Score {
    correct: u32,
    incorrect: u32,
}

impl Score {
    fn increment_correct(&self) -> Self {
        Self {
            correct: self.correct + 1,
            incorrect: self.incorrect,
        }
    }

    fn increment_incorrect(&self) -> Self {
        Self {
            correct: self.correct,
            incorrect: self.incorrect + 1,
        }
    }

    fn correct_rate(&self) -> f32 {
        let tries = self.correct + self.incorrect;
        if tries == 0 {
            1.0
        } else {
            self.correct as f32 / tries as f32
        }
    }

    fn total_tries(&self) -> u32 {
        self.correct + self.incorrect
    }
}

struct OrdinalNum(u32);

impl fmt::Display for OrdinalNum {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)?;
        match self.0 {
            1 => f.write_str("st")?,
            2 => f.write_str("nd")?,
            3 => f.write_str("rd")?,
            _ => f.write_str("th")?,
        }
        Ok(())
    }
}

struct GameState {
    entries: Vec<Rc<Entry>>,
    scores: Scores,
    progress: usize,
    mistakes: usize,
}

impl GameState {
    fn new(entries: Vec<Rc<Entry>>, scores: Scores) -> Self {
        Self {
            entries,
            scores,
            progress: 0,
            mistakes: 0,
        }
    }

    fn next_question(&mut self) -> Option<Question> {
        if self.progress < self.entries.len() {
            let i = self.progress;
            self.progress += 1;
            self.mistakes = 0;
            Some(Question {
                index: i,
                entry: self.entries[i].clone(),
            })
        } else {
            None
        }
    }

    fn answer_question(&mut self, question: &Question, answer: String) -> bool {
        use std::collections::hash_map::Entry;
        let is_correct = question.entry.term == answer;
        if is_correct {
            match self.scores.entry(answer) {
                Entry::Occupied(mut entry) => {
                    let score = if self.mistakes == 0 {
                        entry.get().increment_correct()
                    } else {
                        entry.get().increment_incorrect()
                    };
                    entry.insert(score);
                }
                Entry::Vacant(entry) => {
                    let score = if self.mistakes == 0 {
                        Score {
                            correct: 1,
                            incorrect: 0,
                        }
                    } else {
                        Score {
                            correct: 0,
                            incorrect: 1,
                        }
                    };
                    entry.insert(score);
                }
            }
        } else {
            self.mistakes += 1;
        }
        is_correct
    }

    fn get_score(&self, term: &str) -> Option<Score> {
        self.scores.get(term).cloned()
    }
}

enum UIResponse {
    Return(String),
    Quit,
}

fn load_entries<R: Read>(handle: R) -> io::Result<Vec<Rc<Entry>>> {
    let reader = BufReader::new(handle);
    let mut entries = vec![];
    for line in reader.lines() {
        if let Some(entry) = Entry::parse(line?.chars().peekable()) {
            entries.push(Rc::new(entry))
        }
    }
    Ok(entries)
}

fn load_scores<P: AsRef<Path>>(path: P) -> io::Result<Scores> {
    let mut scores = HashMap::new();
    if path.as_ref().exists() {
        let file = File::open(path)?;
        let reader = BufReader::new(file);
        for line in reader.lines() {
            let line = line?;
            let mut parts = line.split('\t');
            if let Some(term) = parts.next() {
                let score = Score {
                    correct: parts
                        .next()
                        .and_then(|part| str::parse(part).ok())
                        .unwrap_or(0),
                    incorrect: parts
                        .next()
                        .and_then(|part| str::parse(part).ok())
                        .unwrap_or(0),
                };
                scores.insert(term.to_owned(), score);
            }
        }
    }
    Ok(scores)
}

fn save_scores<P: AsRef<Path>>(path: P, scores: Scores) -> io::Result<()> {
    if let Some(parent) = path.as_ref().parent() {
        fs::create_dir_all(parent)?;
    }
    let file = OpenOptions::new().write(true).create(true).open(path)?;
    let mut writer = BufWriter::new(file);
    for (term, score) in scores {
        writeln!(writer, "{}\t{}\t{}", term, score.correct, score.incorrect)?;
    }
    Ok(())
}

fn detect_config_directory() -> PathBuf {
    env::var("XDG_CONFIG_HOME")
        .map(|config_home| Path::new(&config_home).to_path_buf())
        .or_else(|_| env::var("HOME").map(|home_dir| Path::new(&home_dir).join(".config")))
        .unwrap_or_else(|_| env::temp_dir())
        .join("vocab-trainer")
}

fn run_loop(ui: &mut GameUI, state: &mut GameState) -> Result<(), ReadlineError> {
    'outer: while let Some(question) = state.next_question() {
        ui.notify_question(&question, &state);

        loop {
            let hint = QuestionHint {
                entry: question.entry.clone(),
                mistakes: state.mistakes,
            };
            match ui.wait_for_input(hint)? {
                UIResponse::Return(input) => {
                    if state.answer_question(&question, input) {
                        ui.notify_correct(&question, &state);
                        break;
                    } else {
                        ui.notify_incorrect(&question, &state);
                    }
                }
                UIResponse::Quit => break 'outer,
            }
        }
    }
    Ok(())
}

fn main() {
    let config_dir = detect_config_directory();
    let score_path = config_dir.join("scores.txt");
    let entries = load_entries(io::stdin()).expect("load entries");
    let scores = load_scores(&score_path).expect("load scores");
    let mut state = GameState::new(entries, scores);
    let mut ui = GameUI::new();
    run_loop(&mut ui, &mut state).expect("run loop");
    save_scores(&score_path, state.scores).expect("save scores");
}
