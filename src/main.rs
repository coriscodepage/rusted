use std::io;

use crate::repl::Repl;

mod buffer;
mod repl;
mod state;

fn main() {
    let stdio = io::stdin();
    let input = stdio.lock();
    let output = io::stdout();
    Repl::begin(input, output).expect("REPL encountered a fatal error, bailing.");
}
