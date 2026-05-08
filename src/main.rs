use crate::repl::Repl;


mod state;
mod repl;
mod editor;

fn main() {
    Repl::begin();
}
