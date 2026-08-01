use alloc::{
    boxed::Box,
    collections::{BTreeMap, VecDeque},
};
use core::pin::Pin;

use wie_util::Result;

use crate::Instant;

#[allow(clippy::upper_case_acronyms, non_camel_case_types)]
#[derive(Debug, Copy, Clone, Eq, PartialEq, Hash)]
pub enum KeyCode {
    UP,
    DOWN,
    LEFT,
    RIGHT,
    OK,
    LEFT_SOFT_KEY,
    RIGHT_SOFT_KEY,
    CLEAR,
    CALL,
    HANGUP,
    VOLUME_UP,
    VOLUME_DOWN,

    NUM0,
    NUM1,
    NUM2,
    NUM3,
    NUM4,
    NUM5,
    NUM6,
    NUM7,
    NUM8,
    NUM9,
    HASH,
    STAR,
}

impl KeyCode {
    // TODO we can use libraries like strum
    pub fn parse(string: &str) -> KeyCode {
        match string {
            "UP" => KeyCode::UP,
            "DOWN" => KeyCode::DOWN,
            "LEFT" => KeyCode::LEFT,
            "RIGHT" => KeyCode::RIGHT,
            "OK" => KeyCode::OK,
            "0" => KeyCode::NUM0,
            "1" => KeyCode::NUM1,
            "2" => KeyCode::NUM2,
            "3" => KeyCode::NUM3,
            "4" => KeyCode::NUM4,
            "5" => KeyCode::NUM5,
            "6" => KeyCode::NUM6,
            "7" => KeyCode::NUM7,
            "8" => KeyCode::NUM8,
            "9" => KeyCode::NUM9,
            "#" => KeyCode::HASH,
            "*" => KeyCode::STAR,
            "CLR" => KeyCode::CLEAR,
            _ => unimplemented!("Unknown key: {string}"),
        }
    }
}

type TimerCallback = Box<dyn FnOnce() -> Pin<Box<dyn Future<Output = Result<()>> + Send>> + Send + Sync>;

pub enum Event {
    Redraw,
    Keydown(KeyCode),
    Keyup(KeyCode),
    Keyrepeat(KeyCode),
    Timer {
        id: u32,
        generation: u64,
        due: Instant,
        callback: TimerCallback,
    },
    Notify {
        r#type: i32,
        param1: i32,
        param2: i32,
    }, // wipi notifyEvent
}

impl Event {
    fn timer<F, Fut>(id: u32, generation: u64, due: Instant, callback: F) -> Self
    where
        F: FnOnce() -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<()>> + Send + 'static,
    {
        Event::Timer {
            id,
            generation,
            due,
            callback: Box::new(move || Box::pin(callback())),
        }
    }
}

#[derive(Default)]
pub struct EventQueue {
    events: VecDeque<Event>,
    timer_generations: BTreeMap<u32, u64>,
    next_timer_generation: u64,
}

impl EventQueue {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push(&mut self, event: Event) {
        self.events.push_back(event);
    }

    pub fn pop(&mut self) -> Option<Event> {
        self.events.pop_front()
    }

    pub fn push_timer<F, Fut>(&mut self, id: u32, due: Instant, callback: F)
    where
        F: FnOnce() -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<()>> + Send + 'static,
    {
        self.cancel_timer(id);

        self.next_timer_generation = self.next_timer_generation.wrapping_add(1);
        if self.next_timer_generation == 0 {
            self.next_timer_generation = 1;
        }

        let generation = self.next_timer_generation;
        self.timer_generations.insert(id, generation);
        self.events.push_back(Event::timer(id, generation, due, callback));
    }

    pub fn cancel_timer(&mut self, id: u32) {
        self.timer_generations.remove(&id);
        self.events
            .retain(|event| !matches!(event, Event::Timer { id: event_id, .. } if *event_id == id));
    }

    pub fn is_timer_current(&self, id: u32, generation: u64) -> bool {
        self.timer_generations.get(&id).copied() == Some(generation)
    }

    pub fn take_timer(&mut self, id: u32, generation: u64) -> bool {
        if !self.is_timer_current(id, generation) {
            return false;
        }

        self.timer_generations.remove(&id);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::{Event, EventQueue};
    use crate::Instant;

    fn timer_identity(event: &Event) -> (u32, u64) {
        match event {
            Event::Timer { id, generation, .. } => (*id, *generation),
            _ => panic!("expected timer event"),
        }
    }

    #[test]
    fn timer_generation_invalidates_cancelled_and_replaced_events() {
        let mut queue = EventQueue::new();

        queue.push_timer(7, Instant::from_epoch_millis(10), || async { Ok(()) });
        let first = queue.pop().expect("first timer");
        let (first_id, first_generation) = timer_identity(&first);

        assert!(queue.is_timer_current(first_id, first_generation));

        queue.cancel_timer(first_id);
        assert!(!queue.is_timer_current(first_id, first_generation));
        assert!(!queue.take_timer(first_id, first_generation));

        queue.push_timer(7, Instant::from_epoch_millis(20), || async { Ok(()) });
        let second = queue.pop().expect("second timer");
        let (second_id, second_generation) = timer_identity(&second);

        queue.push_timer(7, Instant::from_epoch_millis(30), || async { Ok(()) });
        let third = queue.pop().expect("third timer");
        let (third_id, third_generation) = timer_identity(&third);

        assert_eq!(second_id, third_id);
        assert_ne!(second_generation, third_generation);
        assert!(!queue.is_timer_current(second_id, second_generation));
        assert!(!queue.take_timer(second_id, second_generation));

        assert!(queue.is_timer_current(third_id, third_generation));
        assert!(queue.take_timer(third_id, third_generation));
        assert!(!queue.is_timer_current(third_id, third_generation));
        assert!(!queue.take_timer(third_id, third_generation));
    }
}
