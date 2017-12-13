//! Main bot types

use zephyr::*;
use command::*;

use std::mem;
use std::thread;
use std::time::Duration;
use std::cell::{Ref, RefCell};

/// Represents a bot
pub struct Bot<E = ()> {
    pub state: State<E>,
    pub commands: Vec<Command<E>>,
    pub pre_command_handlers: Vec<Handler<E>>,
    pub post_command_handlers: Vec<Handler<E>>,
}

impl Bot {
    pub fn build(name: &str, start: (&str, &str)) -> builder::Builder {
        builder::Builder::new(name, start)
    }
}

impl<E> Bot<E> {

    pub fn new(
        name: &str,
        class: &str,
        instance: &str,
        zsig_func: Box<Fn() -> String>,
        extra: E,
        subs: Vec<Triplet>,
        commands: Vec<Command<E>>,
        pre_command_handlers: Vec<Handler<E>>,
        post_command_handlers: Vec<Handler<E>>,
    ) -> Bot<E> {
        Bot {
            state: State {
                name: name.to_string(),
                class: class.to_string(),
                instance: instance.to_string(),
                zsig_func,
                extra,
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
pub struct State<E> {
    pub name: String,
    pub class: String,
    pub instance: String,
    zsig_func: Box<Fn() -> String>,
    extra: E,
    zio: RefCell<Zephyr>,
}

impl<E> State<E> {

    pub fn subs(&self) -> Ref<Vec<Triplet>> {
        Ref::map(self.zio.borrow(), |x| x.subs())
    }

    pub fn zwrite(&self, notice: &Notice) {
        self.zio.borrow_mut().zwrite(&notice).expect("unable to send zephyr")
    }

    pub fn reply_here(&self, body: &str) {
        self.reply_at(&self.location(), body)
    }

    pub fn reply_here_zsigned(&self, zsig: &str, body: &str) {
        self.reply_at_zsigned(&self.location(), zsig, body)
    }

    pub fn reply_to(&self, notice: &Notice, body: &str) {
        self.reply_at(&notice.triplet(), body)
    }

    pub fn reply_to_zsigned(&self, notice: &Notice, zsig: &str, body: &str) {
        self.reply_at_zsigned(&notice.triplet(), zsig, body)
    }

    pub fn reply_at(&self, triplet: &Triplet, body: &str) {
        self.reply_at_zsigned(triplet, &(self.zsig_func)(), body)
    }

    pub fn reply_at_zsigned(&self, triplet: &Triplet, zsig: &str, body: &str) {
        let reply = triplet.make_reply(
            &self.name,
            zsig,
            body
        );
        self.zwrite(&reply);
    }

    pub fn location(&self) -> Triplet {
        Triplet::of_instance(&self.class, &self.instance)
    }

    pub fn move_to(&mut self, to: Triplet) {
        self.class = to.class;
        self.instance = to.instance.unwrap_or("personal".to_string());
    }

    pub fn extra_ref(&self) -> &E {
        &self.extra
    }

    pub fn extra_mut(&mut self) -> &mut E {
        &mut self.extra
    }
}

pub mod builder {

    use super::*;

    use rand;
    use rand::Rng;

    pub struct Builder<E = ()> {
        name: String,
        class: String,
        instance: String,
        zsig_func: Box<Fn() -> String>,
        extra: Box<E>,
        subs: Vec<Triplet>,
        commands: Vec<Command<E>>,
        pre_command_handlers: Vec<Handler<E>>,
        post_command_handlers: Vec<Handler<E>>,
    }

    impl Builder {
        pub fn new(name: &str, start: (&str, &str)) -> Builder {
            Builder {
                name: name.to_string(),
                class: start.0.to_string(),
                instance: start.1.to_string(),
                zsig_func: Box::new(|| "".to_string()),
                extra: Box::new(()),
                subs: vec![],
                commands: vec![],
                pre_command_handlers: vec![],
                post_command_handlers: vec![],
            }
        }
    }

    impl<E> Builder<E> {

        pub fn with_zsig(mut self, zsig: &str) -> Builder<E> {
            let owned = zsig.to_string();
            self.zsig_func = Box::new(move || owned.clone());
            self
        }

        pub fn with_zsigs(mut self, zsigs: Vec<&str>) -> Builder<E> {
            assert!(!zsigs.is_empty());
            let owned = zsigs.iter().map(|s| s.to_string()).collect::<Vec<_>>();
            self.zsig_func = Box::new(move || rand::thread_rng().choose(&owned).unwrap().clone());
            self
        }

        pub fn zsig_fn<F>(mut self, f: F) -> Builder<E>
            where F: Fn() -> String + 'static {
            self.zsig_func = Box::new(f);
            self
        }

        pub fn sub_to_class(mut self, class: &str) -> Builder<E> {
            self.subs.push(Triplet::of_class(class));
            self
        }

        pub fn sub_to_classes(mut self, classes: Vec<&str>) -> Builder<E> {
            self.subs.append(&mut classes.iter().map(|c| Triplet::of_class(c)).collect::<Vec<_>>());
            self
        }

        pub fn sub_to(mut self, mut triplets: Vec<Triplet>) -> Builder<E> {
            self.subs.append(&mut triplets);
            self
        }

        pub fn command<F>(mut self, shape: Shape, scope: Scope, labels: Vec<&str>, action: F) -> Builder<E>
            where F: Fn(&mut State<E>, &Notice, &CommandMatch) -> () + 'static {
            self.commands.push(Command::new(shape, scope, labels, action));
            self
        }

        pub fn pre<F>(mut self, action: F) -> Builder<E>
            where F: Fn(&mut State<E>, &Notice) -> bool + 'static {
            self.pre_command_handlers.push(Handler::new(action));
            self
        }

        pub fn post<F>(mut self, action: F) -> Builder<E>
            where F: Fn(&mut State<E>, &Notice) -> bool + 'static {
            self.post_command_handlers.push(Handler::new(action));
            self
        }

        pub fn with_extra<E2>(mut self, extra: E2) -> Builder<E2> {
            let mut extra_box = Box::new(extra);
            unsafe {
                let mut new_builder: Builder<E2> = mem::transmute(self);
                mem::swap(&mut new_builder.extra, &mut extra_box);
                drop(mem::transmute::<Box<E2>, Box<E>>(extra_box));
                new_builder
            }
        }

        pub fn build(self) -> Bot<E> {
            Bot::new(
                &self.name,
                &self.class,
                &self.instance,
                self.zsig_func,
                *self.extra,
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