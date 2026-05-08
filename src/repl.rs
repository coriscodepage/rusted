use std::{ fs::File, io::{ self, Write } };

use crate::{
    editor::{ Buffer, Snapshot },
    state::{ Command, CommandFlow, CommandKind, EdError, Parser },
};

pub struct Repl {
    flow: CommandFlow,
    buffer: Buffer,
    parser: Parser,
    postfix_command: Option<CommandKind>,
    current_file: Option<String>,
    snapshot: Option<Snapshot>,
    exit_confirm: u32,
}

impl Repl {
    pub fn begin() {
        let mut repl = Repl {
            flow: CommandFlow::Command,
            buffer: Buffer::new(),
            parser: Parser::new(),
            postfix_command: None,
            current_file: None,
            snapshot: None,
            exit_confirm: 0,
        };
        let handle = io::stdin();
        loop {
            let mut line = String::new();
            handle.read_line(&mut line).expect("Failed to read line");
            if line.trim() == "." && repl.flow != CommandFlow::Command {
                repl.flow = CommandFlow::Command;
                if let Some(kind) = repl.postfix_command.take() {
                    if Self::match_cmd(&mut repl, Command::empty(kind)).is_err() {
                        Self::print_err();
                    }
                }
                continue;
            }
            match repl.flow {
                CommandFlow::Command => if repl.command_flow(&line).is_err() {
                    Self::print_err();
                }
                CommandFlow::Input => repl.input_flow(&line),
            }
        }
    }

    fn command_flow(&mut self, line: &str) -> Result<(), EdError> {
        match self.parser.parse(&line) {
            Ok(command) => self.match_cmd(command)?,
            Err(e) => {
                return Err(e);
            }
        }
        Ok(())
    }

    fn match_cmd(&mut self, command: Command) -> Result<(), EdError> {
        if command.kind != CommandKind::Quit {
            self.exit_confirm = 0;
        }
        match command.kind {
            CommandKind::Quit => {
                self.exit_confirm += 1;
                if self.snapshot.is_none() || self.exit_confirm == 2 {
                    std::process::exit(0);
                } else {
                    Self::print_notify();
                }
            }
            CommandKind::Append => {
                let snapshot = self.buffer.save_snapshot();
                self.buffer.set_mode(&command.address);
                self.postfix_command = command.suffix;
                self.flow = CommandFlow::Input;
                self.snapshot = Some(snapshot);
            }
            CommandKind::Write(name) => {
                if name.is_some() {
                    self.current_file = name;
                }
                let name = self.current_file.as_ref().ok_or(EdError::InvalidFilename)?;
                let mut file = File::create(name)?;
                let content = self.buffer.get_lines_for_save(&command.address)?;
                let bytes = content.as_bytes();
                let length = bytes.len();
                file.write_all(bytes)?;
                self.snapshot = None;
                Self::print_message(&format!("{}", length));
            }
            CommandKind::Edit(_) => todo!(),
            CommandKind::List => {
                self.buffer.set_range(&command.address);
                print!("{}", self.buffer.get_lines()?.well_defined());
            }
            CommandKind::Delete => {
                let snapshot = self.buffer.save_snapshot();
                self.buffer.set_range(&command.address);
                self.buffer.delete()?;
                self.snapshot = Some(snapshot);
            }
            CommandKind::InternalListLastAffectedLine => {
                self.buffer.set_range(&command.address);
                print!("{}", self.buffer.get_active_line()?);
            }
            CommandKind::Undo => {
                if let Some(restore) = self.snapshot.take() {
                    let snapshot = self.buffer.save_snapshot();
                    self.buffer.restore_snapshot(restore);
                    self.snapshot = Some(snapshot)    
                }
            },
        }
        Ok(())
    }

    fn input_flow(&mut self, line: &str) {
        self.buffer.append(line);
    }

    fn print_message(message: &str) {
        println!("{}", message)
    }

    fn print_notify() {
        println!("?")
    }

    fn print_err() {
        println!("?")
    }
}
