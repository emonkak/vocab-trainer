#![allow(non_camel_case_types, non_snake_case, non_upper_case_globals, unused)]

use nix::libc::FILE;

include!(concat!(env!("OUT_DIR"), "/readline_sys.rs"));