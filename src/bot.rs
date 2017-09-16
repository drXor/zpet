//! Main bot types

use zephyr::*;
use command::*;

use std::thread;
use std::time::Duration;

/// Represents a bot
pub struct Bot {
    pub state: State,
    pub commands: Vec<Command>,
}

impl Bot {

    pub fn new(name: &str, class: &str, instance: &str, subs: Vec<Triplet>, commands: Vec<Command>) -> Bot {
        Bot {
            state: State {
                name: name.to_string(),
                class: class.to_string(),
                instance: instance.to_string(),
                zio: Zephyr::new(subs).expect("failed to connect to Zephyr")
            },
            commands,
        }
    }

    pub fn run(&mut self) {
        loop {
            match self.state.zio.read() {
                Ok(notice) => {
                    self.tick(notice);
                    thread::sleep(Duration::from_millis(100))
                },
                Err(e) => eprintln!("{:?}", e),
            }

        }
    }

    pub fn tick(&mut self, notice: Notice) {

        if notice.opcode == "AUTO" { return }

        for cmd in self.commands.iter_mut() {
            if cmd.try_exec(&mut self.state, &notice) {
                break
            }
        }
    }
}

/// represents the mutable state of a bot
pub struct State {
    pub name: String,
    pub class: String,
    pub instance: String,
    zio: Zephyr,
}

impl State {

    pub fn zephyr(&self) -> &Zephyr {
        &self.zio
    }

    pub fn zephyr_mut(&mut self) -> &mut Zephyr {
        &mut self.zio
    }
}