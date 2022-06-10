extern crate rustyline;
extern crate rustyline_derive;
extern crate termion;

use std::borrow::Cow;
use std::io::{self, BufRead as _, BufReader, Read};
use std::iter::Peekable;
use std::mem;
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
    tries: usize,
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
                } else if i - symbols < self.tries {
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

struct GameLoop {
    state: GameState,
    ui: GameUI,
}

impl GameLoop {
    fn run(&mut self) {
        'outer: while let Some(question) = self.state.next_question() {
            let mut tries = 0;

            self.ui.notify_question(&question);

            loop {
                let hint = QuestionHint {
                    entry: question.entry.clone(),
                    tries,
                };
                match self.ui.wait_for_input(hint) {
                    UIResponse::Return(input) => {
                        if self.state.answer_question(&question, &input) {
                            self.ui.notify_correct(&question);
                            break;
                        } else {
                            self.ui.notify_incorrect(&question);
                            tries += 1;
                        }
                    }
                    UIResponse::Error(error) => {
                        self.ui.notify_error(error);
                        break 'outer;
                    }
                    UIResponse::Quit => break 'outer,
                }
            }
        }
    }
}

struct GameState {
    entries: Vec<Rc<Entry>>,
    progress: usize,
}

impl GameState {
    fn new(entries: Vec<Rc<Entry>>) -> Self {
        Self {
            entries,
            progress: 0,
        }
    }

    fn next_question(&mut self) -> Option<Question> {
        if self.progress < self.entries.len() {
            let i = self.progress;
            self.progress += 1;
            Some(Question {
                index: i,
                entry: self.entries[i].clone(),
            })
        } else {
            None
        }
    }

    fn answer_question(&mut self, question: &Question, answer: &str) -> bool {
        let is_correct = question.entry.term == answer;
        // TODO: Record scores
        is_correct
    }
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

    fn notify_question(&mut self, question: &Question) {
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

    fn notify_correct(&mut self, _question: &Question) {}

    fn notify_incorrect(&mut self, _question: &Question) {
        println!(
            "{}{}{}",
            termion::cursor::Up(1),
            termion::clear::CurrentLine,
            termion::cursor::Up(1),
        );
    }

    fn notify_error(&mut self, error: ReadlineError) {
        eprintln!("Error: {}", error);
    }

    fn wait_for_input(&mut self, hint: QuestionHint) -> UIResponse {
        self.readline.set_helper(Some(hint));
        match self.readline.readline("> ") {
            Ok(input) if input.starts_with(":") => {
                if input
                    .get(1..)
                    .map_or(false, |input| "quit".starts_with(input))
                {
                    UIResponse::Quit
                } else {
                    UIResponse::Return(input)
                }
            }
            Ok(input) => UIResponse::Return(input),
            Err(ReadlineError::Interrupted | ReadlineError::Eof) => UIResponse::Quit,
            Err(error) => UIResponse::Error(error),
        }
    }
}

enum UIResponse {
    Return(String),
    Error(ReadlineError),
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

fn main() {
    let entries = load_entries(io::stdin()).expect("failed to load entries");
    let mut game_loop = GameLoop {
        state: GameState::new(entries),
        ui: GameUI::new(),
    };
    game_loop.run();
}
