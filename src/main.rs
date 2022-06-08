extern crate vocab_trainer;

use std::ffi::CString;
use std::ffi::IntoStringError;
use vocab_trainer::readline_sys;

fn next_line() -> Result<String, IntoStringError> {
    const PROMPT: &'static str = "> \0";
    let line = unsafe { CString::from_raw(readline_sys::readline(PROMPT.as_ptr().cast())) };
    line.into_string()
}

fn main() {
    loop {
        let line = next_line().unwrap();
        match line.as_str() {
            "exit" | "quit" => break,
            _ => {
                println!("{:?}", line)
            }
        }
    }
}
