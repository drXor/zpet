//! Main bot types

use zephyr::*;
use command::*;

use std::any::Any;
use std::thread;
use std::time::Duration;
use std::collections::HashMap;
use std::cell::{RefCell};

/// Represents a bot
pub struct Bot {
    pub state: State,
    pub commands: Vec<Command>,
    pub pre_command_handlers: Vec<Handler>,
    pub post_command_handlers: Vec<Handler>,
}

impl Bot {

    pub fn build(name: &str, start: (&str, &str)) -> builder::Builder {
        builder::Builder::new(name, start)
    }

    pub fn new(
        name: &str,
        class: &str,
        instance: &str,
        zsig_func: Box<Fn() -> String>,
        initial_data: HashMap<&'static str, Box<Any>>,
        subs: Vec<Triplet>,
        commands: Vec<Command>,
        pre_command_handlers: Vec<Handler>,
        post_command_handlers: Vec<Handler>,
    ) -> Bot {
        Bot {
            state: State {
                name: name.to_string(),
                class: class.to_string(),
                instance: instance.to_string(),
                zsig_func,
                extra: initial_data,
                zio: RefCell::new(Zephyr::new(subs).expect("failed to connect to Zephyr"))
            },
            commands,
            pre_command_handlers,
            post_command_handlers
        }
    }

    pub fn run(&mut self) {
        loop {
            match {
                let mut zio = self.state.zio.borrow_mut();
                let notice = zio.read();
                drop(zio);
                notice
            } {
                Ok(notice) => {
                    self.tick(notice);
                    thread::sleep(Duration::from_millis(100))
                },
                Err(e) => eprintln!("{:?}", e),
            }

        }
    }

    pub fn tick(&mut self, notice: Notice) {

        if notice.opcode == "AUTO" {
            return
        }

        for hdl in self.pre_command_handlers.iter() {
            if hdl.try_exec(&mut self.state, &notice) {
                return
            }
        }

        for cmd in self.commands.iter() {
            if cmd.try_exec(&mut self.state, &notice) {
                return
            }
        }

        for hdl in self.post_command_handlers.iter() {
            if hdl.try_exec(&mut self.state, &notice) {
                return
            }
        }
    }
}

/// Mutable state of a bot. Used by commands and handlers
/// to share state
pub struct State {
    pub name: String,
    pub class: String,
    pub instance: String,
    zsig_func: Box<Fn() -> String>,
    extra: HashMap<&'static str, Box<Any>>,
    zio: RefCell<Zephyr>,
}

impl State {

    pub fn zwrite(&self, notice: &Notice) {
        self.zio.borrow_mut().zwrite(&notice).expect("unable to send zephyr")
    }

    pub fn reply_to(&self, notice: &Notice, body: &str) {
        self.reply_zsigned(notice, &(self.zsig_func)(), body)
    }

    pub fn reply_zsigned(&self, notice: &Notice, zsig: &str, body: &str) {
        let reply = notice.make_reply(
            &self.name,
            zsig,
            body
        );
        self.zwrite(&reply);
    }

    pub fn get_data<T: Any + 'static>(&self, key: &'static str) -> Option<&T> {
        if let Some(x) = self.extra.get(key) {
            if let Some(y) = x.downcast_ref::<T>() {
                return Some(y)
            }
        }
        None
    }

    pub fn check_data<T: Eq + 'static>(&self, key: &'static str, other: &T) -> bool {
        self.get_data(key).map(|x: &T| *x == *other).unwrap_or(false)
    }

    pub fn insert_data<T: Any + Clone + 'static>(&mut self, key: &'static str, t: &T) -> Option<Box<Any>> {
        self.extra.insert(key, Box::new(t.clone()))
    }

    pub fn remove_data(&mut self, key: &'static str) -> Option<Box<Any>> {
        self.extra.remove(key)
    }

    pub fn move_to(&mut self, to: Triplet) {
        self.class = to.class;
        self.instance = to.instance.unwrap_or("personal".to_string());
    }
}

pub mod builder {

    use super::*;
    use zephyr::*;
    use command::*;

    use rand;
    use rand::Rng;

    use std::any::Any;
    use std::collections::HashMap;

    pub struct Builder {
        name: String,
        class: String,
        instance: String,
        zsig_func: Box<Fn() -> String>,
        initial_data: HashMap<&'static str, Box<Any>>,
        subs: Vec<Triplet>,
        commands: Vec<Command>,
        pre_command_handlers: Vec<Handler>,
        post_command_handlers: Vec<Handler>,
    }

    impl Builder {

        pub fn new(name: &str, start: (&str, &str)) -> Builder {
            Builder {
                name: name.to_string(),
                class: start.0.to_string(),
                instance: start.1.to_string(),
                zsig_func: Box::new(|| "".to_string()),
                initial_data: HashMap::new(),
                subs: vec![],
                commands: vec![],
                pre_command_handlers: vec![],
                post_command_handlers: vec![],
            }
        }

        pub fn with_zsig(mut self, zsig: &str) -> Builder {
            let owned = zsig.to_string();
            self.zsig_func = Box::new(move || owned.clone());
            self
        }

        pub fn with_zsigs(mut self, zsigs: Vec<&str>) -> Builder {
            assert!(!zsigs.is_empty());
            let owned = zsigs.iter().map(|s| s.to_string()).collect::<Vec<_>>();
            self.zsig_func = Box::new(move || rand::thread_rng().choose(&owned).unwrap().clone());
            self
        }

        pub fn zsig_fn<F>(mut self, f: F) -> Builder
            where F: Fn() -> String + 'static {
            self.zsig_func = Box::new(f);
            self
        }

        pub fn sub_to_class(mut self, class: &str) -> Builder {
            self.subs.push(Triplet::of_class(class));
            self
        }

        pub fn sub_to_classes(mut self, classes: Vec<&str>) -> Builder {
            self.subs.append(&mut classes.iter().map(|c| Triplet::of_class(c)).collect::<Vec<_>>());
            self
        }

        pub fn sub_to(mut self, mut triplets: Vec<Triplet>) -> Builder {
            self.subs.append(&mut triplets);
            self
        }

        pub fn command<F>(mut self, shape: Shape, scope: Scope, labels: Vec<&str>, action: F) -> Builder
            where F: Fn(&mut State, &Notice, &CommandMatch) -> () + 'static {
            self.commands.push(Command::new(shape, scope, labels, action));
            self
        }

        pub fn pre<F>(mut self, action: F) -> Builder
            where F: Fn(&mut State, &Notice) -> bool + 'static {
            self.pre_command_handlers.push(Handler::new(action));
            self
        }

        pub fn post<F>(mut self, action: F) -> Builder
            where F: Fn(&mut State, &Notice) -> bool + 'static {
            self.post_command_handlers.push(Handler::new(action));
            self
        }

        pub fn set<T: 'static>(mut self, key: &'static str, val: T) -> Builder {
            self.initial_data.insert(key, Box::new(val));
            self
        }

        pub fn set_vec<T: 'static>(mut self, pairs: Vec<(&'static str, T)>) -> Builder {
            for (k, v) in pairs.into_iter() {
                self.initial_data.insert(k, Box::new(v));
            }
            self
        }

        pub fn build(self) -> Bot {
            Bot::new(
                &self.name,
                &self.class,
                &self.instance,
                self.zsig_func,
                self.initial_data,
                self.subs,
                self.commands,
                self.pre_command_handlers,
                self.post_command_handlers
            )
        }

        pub fn run(self) {
            self.build().run()
        }
    }
}