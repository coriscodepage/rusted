use crate::repl::Repl;

mod buffer;
mod repl;
mod state;

fn main() {
    Repl::begin();
}
