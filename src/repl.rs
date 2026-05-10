use std::{
    fs::File,
    io::{BufRead, Read, Write},
    ops::ControlFlow,
};

use regex::Regex;

use crate::{
    buffer::{Buffer, Snapshot},
    state::{Address, Command, CommandFlow, CommandKind, EdError, Parser},
};

pub struct Repl<R, W> {
    flow: CommandFlow,
    buffer: Buffer,
    parser: Parser,
    postfix_command: Option<CommandKind>,
    current_file: Option<String>,
    snapshot: Option<Snapshot>,
    exit_confirm: u32,
    edit_confirm: u32,
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
            edit_confirm: 0,
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
        self.edit_confirm = 0;
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
                repl.buffer.exit_mode()?;
                repl.run_postfix()?;
                continue;
            }
            match repl.flow {
                CommandFlow::Command => {
                    if repl.command_flow(&line).is_err() {
                        repl.print_err()?;
                    }
                    if repl.flow != CommandFlow::Input {
                        repl.run_postfix()?;
                    }
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

    fn save_snapshot(&mut self) -> Snapshot {
        self.edit_confirm = 0;
        self.buffer.save_snapshot()
    }

    fn match_cmd(&mut self, command: Command) -> Result<(), EdError> {
        if !matches!(command.kind, CommandKind::Quit) {
            self.exit_confirm = 0;
        }
        match command.kind {
            CommandKind::Quit => {
                if !matches!(command.address, Address::None) {
                    return Err(EdError::InvalidAddress);
                }
                self.exit_confirm += 1;
                if self.snapshot.is_none() || self.exit_confirm == 2 {
                    self.control_flow = ControlFlow::Break(());
                } else {
                    self.print_notify()?;
                }
            }
            CommandKind::Append => {
                let snapshot = self.save_snapshot();
                self.buffer.set_mode(&command.address)?;
                self.postfix_command = command.suffix;
                self.flow = CommandFlow::Input;
                self.snapshot = Some(snapshot);
            }
            CommandKind::Change => {
                let snapshot = self.save_snapshot();
                self.postfix_command = command.suffix;
                self.buffer.set_range(&command.address)?;
                self.yank_buffer = Some(self.buffer.delete()?);
                self.buffer.adj_change()?;
                self.flow = CommandFlow::Input;
                self.snapshot = Some(snapshot);
            }
            CommandKind::Write(name) => {
                if self.current_file.is_none() {
                    self.current_file = name.clone();
                }
                let name = name
                    .as_ref()
                    .or(self.current_file.as_ref())
                    .ok_or(EdError::InvalidFilename)?;
                let mut file = File::create(name)?;
                let content = self.buffer.get_lines_for_save(&command.address)?;
                let bytes = content.as_bytes();
                let length = bytes.len();
                file.write_all(bytes)?;
                self.snapshot = None;
                self.print_message(&format!("{}", length))?;
            }
            CommandKind::Edit(name) => {
                self.edit_confirm += 1;
                if self.snapshot.is_none() || self.edit_confirm == 2 {
                    let name = name
                        .or(self.current_file.take())
                        .ok_or(EdError::InvalidFilename)?;
                    let mut file = File::open(&name)?;
                    let mut contents = String::new();
                    file.read_to_string(&mut contents)?;
                    self.reset();
                    self.current_file = Some(name);
                    contents.lines().for_each(|l| self.buffer.append(l));
                    self.print_message(&format!("{}", contents.len()))?;
                } else {
                    return Err(EdError::EndOfInput);
                }
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
                let snapshot = self.save_snapshot();
                self.buffer.set_range(&command.address)?;
                self.yank_buffer = Some(self.buffer.delete()?);
                self.postfix_command = command.suffix;
                self.snapshot = Some(snapshot);
            }
            CommandKind::InternalListLastAffectedLine => {
                self.buffer.set_range(&command.address)?;
                write!(self.writer, "{}", self.buffer.get_active_line()?)?;
            }
            CommandKind::Undo => {
                if !matches!(command.address, Address::None) {
                    return Err(EdError::InvalidAddress);
                }
                if let Some(restore) = self.snapshot.take() {
                    let snapshot = self.save_snapshot();
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
                    .cloned()
                    .or_else(|| self.current_file.clone())
                    .ok_or(EdError::InvalidFilename)?;
                let snapshot = self.save_snapshot();
                let mut file = File::open(&name)?;
                let mut contents = String::new();
                file.read_to_string(&mut contents)?;
                self.buffer.set_mode(&command.address)?;
                contents.lines().for_each(|l| self.buffer.append(l));
                self.snapshot = Some(snapshot);
                self.print_message(&format!("{}", contents.len()))?;
            }
            CommandKind::Insert => {
                let snapshot = self.save_snapshot();
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
                let snapshot = self.save_snapshot();
                self.buffer.set_range(&command.address)?;
                self.buffer.transfer(&address)?;
                self.snapshot = Some(snapshot);
            }
            CommandKind::Put => {
                if self.yank_buffer.is_some() {
                    let yank_buffer = self.yank_buffer.clone().unwrap();
                    let snapshot = self.save_snapshot();
                    self.buffer.set_mode(&command.address)?;
                    yank_buffer.iter().for_each(|l| self.buffer.append(l));
                    self.snapshot = Some(snapshot);
                } else {
                    return Err(EdError::NoData);
                }
            }
            CommandKind::Move(address) => {
                let snapshot = self.save_snapshot();
                self.buffer.set_range(&command.address)?;
                self.buffer.move_to(&address)?;
                self.snapshot = Some(snapshot);
            }
            CommandKind::Join => {
                let snapshot = self.save_snapshot();
                self.buffer.join(&command.address)?;
                self.snapshot = Some(snapshot);
            }

            CommandKind::NoOp => {}
            CommandKind::MultiLineCommand(_) => todo!(),
            CommandKind::Substitution {
                re,
                mut sub,
                flag,
                repeated,
            } => {
                let mut suffix = command.suffix;
                let snapshot = self.save_snapshot();
                let mut global_flag = false;
                let mut nth = 0;
                let mut remembered_flag = None;
                let mut update_remembered_flag = false;

                if repeated {
                    match flag {
                        Some(f) if f == "g" => {
                            update_remembered_flag = true;
                            if self.buffer.last_flag.as_deref() == Some("g") {
                                remembered_flag = Some(String::from("de"));
                            } else {
                                remembered_flag = Some(f);
                                global_flag = true;
                            }
                        }
                        Some(f) if f == "0" => return Err(EdError::InvalidAddress),
                        Some(f) => {
                            update_remembered_flag = true;
                            nth = f.parse().unwrap_or(0);
                            remembered_flag = Some(f);
                        }
                        None => match self.buffer.last_flag.as_deref() {
                            Some("g") => global_flag = true,
                            Some(n) => nth = n.parse().unwrap_or(0),
                            None => {}
                        },
                    }
                } else {
                    match flag {
                        Some(f) if f == "g" => {
                            update_remembered_flag = true;
                            remembered_flag = Some(f);
                            global_flag = true;
                        }
                        Some(f) if f == "0" => return Err(EdError::InvalidAddress),
                        Some(f) => {
                            update_remembered_flag = true;
                            nth = f.parse().unwrap_or(0);
                            remembered_flag = Some(f);
                        }
                        None => {
                            update_remembered_flag = true;
                            remembered_flag = Some(String::from("de"));
                            if suffix.is_none() {
                                suffix = Some(CommandKind::PrintList)
                            }
                        }
                    }
                }

                let re = match re {
                    Some(re) => {
                        self.buffer.last_re = Some(re);
                        self.buffer
                            .last_re
                            .as_deref()
                            .ok_or(EdError::RegexNotFound)?
                    }
                    None => self
                        .buffer
                        .last_re
                        .as_deref()
                        .ok_or(EdError::RegexNotFound)?,
                };
                let re = Regex::new(re)?;
                self.buffer.set_range(&command.address)?;
                let mut previous_sub = self.buffer.last_sub.take();
                if repeated {
                    sub = previous_sub.take().ok_or(EdError::NoData)?;
                }
                let use_previous_sub = sub.len() == 1 && sub[0] == "%";
                if use_previous_sub && !repeated && previous_sub.is_none() {
                    self.buffer.last_sub = previous_sub;
                    return Err(EdError::NoData);
                }

                let mut success = false;
                let mut changes = Vec::new();

                {
                    let replacement = if use_previous_sub {
                        previous_sub.as_deref().unwrap_or(sub.as_slice())
                    } else {
                        sub.as_slice()
                    };

                    let lines = match self.buffer.get_lines() {
                        Ok(lines) => lines,
                        Err(e) => {
                            self.buffer.last_sub = if repeated { Some(sub) } else { previous_sub };
                            return Err(e);
                        }
                    };

                    for (line_number, line) in lines.iter() {
                        let mut last = 0;
                        let mut out = String::new();
                        for (index, cap) in re.captures_iter(line.as_str()).enumerate() {
                            if nth > 0 && index + 1 < nth as usize {
                                continue;
                            }

                            let m = cap.get(0).unwrap();
                            out.push_str(&line[last..m.start()]);
                            for part in replacement {
                                let mut chars = part.chars();
                                while let Some(bite) = chars.next() {
                                    match bite {
                                        '&' => out.push_str(m.as_str()),
                                        '\\' => match chars.next() {
                                            Some('&') => out.push('&'),
                                            Some(d @ '1'..='9') => {
                                                let idx = d.to_digit(10).unwrap_or(0) as usize;
                                                out.push_str(
                                                    cap.get(idx).map(|m| m.as_str()).unwrap_or(""),
                                                );
                                            }
                                            Some(ch) => out.push(ch),
                                            None => out.push('\\'),
                                        },
                                        ch => out.push(ch),
                                    };
                                }
                            }
                            success |= true;
                            last = m.end();
                            if !global_flag {
                                break;
                            }
                        }

                        out.push_str(&line[last..]);
                        for (i, line) in out.split_inclusive('\n').enumerate() {
                            changes.push((i == 0, line_number, String::from(line)));
                        }
                    }
                }

                if !success {
                    self.buffer.restore_snapshot(snapshot);
                    self.buffer.last_sub = if repeated { Some(sub) } else { previous_sub };
                    return Err(EdError::RegexNotFound);
                }
                for (t, i, c) in changes {
                    if t {
                        self.buffer.change_line_at(i, c)?;
                    } else {
                        self.buffer.append(&c);
                    }
                }
                self.buffer.last_sub = Some(sub);
                if update_remembered_flag {
                    self.buffer.last_flag = remembered_flag;
                }
                let run_suffix = if repeated {
                    if let Some(new_suffix) = suffix {
                        let toggles_off = matches!(
                            (&new_suffix, &self.buffer.last_suffix),
                            (CommandKind::List, Some(CommandKind::List))
                                | (CommandKind::NumberedList, Some(CommandKind::NumberedList))
                                | (CommandKind::PrintList, Some(CommandKind::PrintList))
                        );
                        if toggles_off {
                            self.buffer.last_suffix = None;
                            false
                        } else {
                            self.buffer.last_suffix = Some(new_suffix);
                            true
                        }
                    } else {
                        self.buffer.last_suffix.is_some()
                    }
                } else {
                    self.buffer.last_suffix = suffix;
                    self.buffer.last_suffix.is_some()
                };
                if run_suffix {
                    if matches!(self.buffer.last_suffix, Some(CommandKind::List)) {
                        self.buffer.set_range(&Address::None)?;
                        write!(self.writer, "{}", self.buffer.get_lines()?.well_defined())?;
                    } else if matches!(self.buffer.last_suffix, Some(CommandKind::NumberedList)) {
                        self.buffer.set_range(&Address::None)?;
                        write!(self.writer, "{}", self.buffer.get_lines()?.numbered())?;
                    } else if matches!(self.buffer.last_suffix, Some(CommandKind::PrintList)) {
                        self.buffer.set_range(&Address::None)?;
                        write!(self.writer, "{}", self.buffer.get_lines()?)?;
                    }
                }
                self.postfix_command = None;
                self.snapshot = Some(snapshot);
            } // _ => panic!("Unexpected Command"),
            CommandKind::File(name) => {
                if let Some(name) = name {
                    self.print_message(&format!("{}", name))?;
                    self.current_file = Some(name);
                } else if let Some(name) = &self.current_file {
                    self.print_message(&format!("{}", name))?;
                } else {
                    return Err(EdError::InvalidFilename);
                }
            }
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
