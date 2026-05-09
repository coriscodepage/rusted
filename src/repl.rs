use std::{
    fs::File,
    io::{BufRead, Read, Write},
    ops::ControlFlow,
};

use regex::Regex;

use crate::{
    buffer::{Buffer, Snapshot},
    state::{Command, CommandFlow, CommandKind, EdError, Parser},
};

pub struct Repl<R, W> {
    flow: CommandFlow,
    buffer: Buffer,
    parser: Parser,
    postfix_command: Option<CommandKind>,
    current_file: Option<String>,
    snapshot: Option<Snapshot>,
    exit_confirm: u32,
    yank_buffer: Option<Vec<String>>,
    control_flow: ControlFlow<()>,
    reader: R,
    writer: W,
}

impl<R, W> Repl<R, W>
where
    R: BufRead,
    W: Write,
{
    fn new(reader: R, writer: W) -> Self {
        Self {
            flow: CommandFlow::Command,
            buffer: Buffer::new(),
            parser: Parser::new(),
            postfix_command: None,
            current_file: None,
            snapshot: None,
            exit_confirm: 0,
            yank_buffer: None,
            control_flow: ControlFlow::Continue(()),
            reader,
            writer,
        }
    }

    fn reset(&mut self) {
        self.flow = CommandFlow::Command;
        self.buffer = Buffer::new();
        self.parser = Parser::new();
        self.postfix_command = None;
        self.current_file = None;
        self.snapshot = None;
        self.exit_confirm = 0;
        self.yank_buffer = None;
        self.control_flow = ControlFlow::Continue(());
    }
    pub fn begin(reader: R, writer: W) -> Result<(), EdError> {
        let mut repl = Repl::new(reader, writer);
        let mut line = String::new();
        loop {
            if repl.control_flow.is_break() {
                break Ok(());
            }
            line.clear();
            repl.reader
                .read_line(&mut line)
                .expect("Failed to read line");
            if line.chars().nth(0).is_some_and(|c| c == '.')
                && line.trim() == "."
                && repl.flow == CommandFlow::Input
            {
                repl.flow = CommandFlow::Command;
                repl.run_postfix()?;
                continue;
            }
            match repl.flow {
                CommandFlow::Command => {
                    repl.command_flow(&line).unwrap(); //.is_err() {
                    //repl.print_err()?;
                    //}
                    repl.run_postfix()?;
                }
                CommandFlow::Input => repl.input_flow(&line),
            }
        }
    }

    fn run_postfix(&mut self) -> Result<(), EdError> {
        if let Some(kind) = self.postfix_command.take() {
            if self.match_cmd(Command::empty(kind)).is_err() {
                self.print_err()?;
            }
        }
        Ok(())
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
        if !matches!(command.kind, CommandKind::Quit) {
            self.exit_confirm = 0;
        }
        match command.kind {
            CommandKind::Quit => {
                self.exit_confirm += 1;
                if self.snapshot.is_none() || self.exit_confirm == 2 {
                    self.control_flow = ControlFlow::Break(());
                } else {
                    self.print_notify()?;
                }
            }
            CommandKind::Append => {
                let snapshot = self.buffer.save_snapshot();
                self.buffer.set_mode(&command.address)?;
                self.postfix_command = command.suffix;
                self.flow = CommandFlow::Input;
                self.snapshot = Some(snapshot);
            }
            CommandKind::Change => {
                let snapshot = self.buffer.save_snapshot();
                self.postfix_command = command.suffix;
                self.buffer.set_range(&command.address)?;
                self.yank_buffer = Some(self.buffer.delete()?);
                self.buffer.adj_change()?;
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
                self.print_message(&format!("{}", length))?;
            }
            CommandKind::Edit(name) => {
                if name.is_some() {
                    self.current_file = name;
                }
                let name = self.current_file.as_ref().ok_or(EdError::InvalidFilename)?;
                let mut file = File::open(name)?;
                let mut contents = String::new();
                file.read_to_string(&mut contents)?;
                self.reset();
                contents.lines().for_each(|l| self.buffer.append(l));
                self.print_message(&format!("{}", contents.len()))?;
            }
            CommandKind::List => {
                self.buffer.set_range(&command.address)?;
                write!(self.writer, "{}", self.buffer.get_lines()?.well_defined())?;
            }
            CommandKind::NumberedList => {
                self.buffer.set_range(&command.address)?;
                write!(self.writer, "{}", self.buffer.get_lines()?.numbered())?;
            }
            CommandKind::Delete => {
                let snapshot = self.buffer.save_snapshot();
                self.buffer.set_range(&command.address)?;
                self.yank_buffer = Some(self.buffer.delete()?);
                self.snapshot = Some(snapshot);
            }
            CommandKind::InternalListLastAffectedLine => {
                self.buffer.set_range(&command.address)?;
                write!(self.writer, "{}", self.buffer.get_active_line()?)?;
            }
            CommandKind::Undo => {
                if let Some(restore) = self.snapshot.take() {
                    let snapshot = self.buffer.save_snapshot();
                    self.buffer.restore_snapshot(restore);
                    self.snapshot = Some(snapshot);
                    self.postfix_command = command.suffix;
                } else {
                    return Err(EdError::NoData);
                }
            }
            CommandKind::PrintList => {
                self.buffer.set_range(&command.address)?;
                write!(self.writer, "{}", self.buffer.get_lines()?)?;
            }
            CommandKind::Read(name) => {
                if self.current_file.is_none() {
                    self.current_file = name.clone();
                }

                let name = name
                    .as_ref()
                    .or(self.current_file.as_ref())
                    .ok_or(EdError::InvalidFilename)?;
                let snapshot = self.buffer.save_snapshot();
                let mut file = File::open(name)?;
                let mut contents = String::new();
                file.read_to_string(&mut contents)?;
                self.buffer.set_mode(&command.address)?;
                contents.lines().for_each(|l| self.buffer.append(l));
                self.snapshot = Some(snapshot);
                self.print_message(&format!("{}", contents.len()))?;
            }
            CommandKind::Insert => {
                let snapshot = self.buffer.save_snapshot();
                self.buffer.set_mode_insert(&command.address)?;
                self.postfix_command = command.suffix;
                self.flow = CommandFlow::Input;
                self.snapshot = Some(snapshot);
            }
            CommandKind::Yank => {
                self.buffer.set_range(&command.address)?;
                let yanked: Vec<String> = self.buffer.get_lines()?.into();
                self.yank_buffer = Some(yanked);
            }
            CommandKind::Transfer(address) => {
                let snapshot = self.buffer.save_snapshot();
                self.buffer.set_range(&command.address)?;
                self.buffer.transfer(&address)?;
                self.snapshot = Some(snapshot);
            }
            CommandKind::Put => {
                if let Some(yank_buffer) = &self.yank_buffer {
                    let snapshot = self.buffer.save_snapshot();
                    self.buffer.set_mode(&command.address)?;
                    yank_buffer.iter().for_each(|l| self.buffer.append(l));
                    self.snapshot = Some(snapshot);
                } else {
                    return Err(EdError::NoData);
                }
            }
            CommandKind::Move(address) => {
                let snapshot = self.buffer.save_snapshot();
                self.buffer.set_range(&command.address)?;
                self.buffer.move_to(&address)?;
                self.snapshot = Some(snapshot);
            }
            CommandKind::Join => {
                let snapshot = self.buffer.save_snapshot();
                self.buffer.set_range(&command.address)?;
                self.buffer.join()?;
                self.snapshot = Some(snapshot);
            }

            CommandKind::NoOP => {}
            CommandKind::MultiLineCommand(multi_line_command) => todo!(),
            CommandKind::Substitution { re, sub, flag } => {
                println!("address: {:?}", command.address);
                println!("regex: {:?}", re);
                println!("substitution: {:?}", sub);
                println!("flag: {:?}", flag);
                if self.buffer.last_re.is_none() {
                    self.buffer.last_re = re;
                } else {
                    let re = re
                        .as_ref()
                        .or(self.buffer.last_re.as_ref())
                        .ok_or(EdError::RegexNotFound)?;
                    let re = Regex::new(&re)?;
                    for line in sub {
                        if let Some(caps) = re.captures(&line) {
                            
                        }
                    }
                    
                }
            }
            // _ => panic!("Unexpected Command"),
        }
        Ok(())
    }

    fn input_flow(&mut self, line: &str) {
        self.buffer.append(line);
    }

    fn print_message(&mut self, message: &str) -> Result<(), EdError> {
        writeln!(self.writer, "{}", message)?;
        Ok(())
    }

    fn print_notify(&mut self) -> Result<(), EdError> {
        writeln!(self.writer, "?")?;
        Ok(())
    }

    fn print_err(&mut self) -> Result<(), EdError> {
        writeln!(self.writer, "?")?;
        Ok(())
    }
}
