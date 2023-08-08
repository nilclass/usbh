//! alternative implementation to `enumeration`

use crate::{
    Event,
    types::ConnectionSpeed,
};

struct Enumerator {
    delay0: u8,
    delay1: u8,
    state: State,
    speed: ConnectionSpeed,
}

enum State {
    WaitForDevice,
    Reset0,
    Delay0(u8),
    WaitDescriptor,
    Reset1,
    Delay1(u8),
    WaitSetAddress,
    Done,
}

enum Action {
    ResetBus,
    EnableSofInterrupt,
    GetDescriptor,
    SetAddress,
    Done,
}

impl Enumerator {
    pub fn new(delay0: u8, delay1: u8) -> Self {
        Self {
            delay0,
            delay1,
            state: State::WaitForDevice,
            // doesn't matter at this point
            speed: ConnectionSpeed::Full,
        }
    }

    pub fn process(&mut self, event: Event) -> Option<Action> {
        use State::*;
        match self.state {
            WaitForDevice => {
                if let Event::Attached(speed) = event {
                    self.speed = speed;
                    self.state = Reset0;
                    return Some(Action::ResetBus);
                }
            }

            Reset0 => {
                if let Event::Attached(speed) = event {
                    self.speed = speed;
                    self.state = Delay0(self.delay0);
                    return Some(Action::EnableSofInterrupt);
                }
            }

            Delay0(n) => {
                if let Event::Sof = event {
                    if n > 0 {
                        self.state = Delay0(n - 1);
                    } else {
                        self.state = WaitDescriptor;
                        return Some(Action::GetDescriptor);
                    }
                }
            }

            WaitDescriptor => {
                if let Event::ControlInData(_, _) = event {
                    self.state = Reset1;
                    return Some(Action::ResetBus)
                }
            },

            Reset1 => {
                if let Event::Attached(speed) = event {
                    self.speed = speed;
                    self.state = Delay1(self.delay1);
                }
            },

            Delay1(n) => {
                if let Event::Sof = event {
                    if n > 0 {
                        self.state = Delay0(n - 1);
                    } else {
                        self.state = WaitSetAddress;
                        return Some(Action::SetAddress);
                    }
                }
            }

            WaitSetAddress => {
                if let Event::ControlInData(_, _) = event {
                    self.state = Done;
                    return Some(Action::Done)
                }
            },

            Done => {},
        }

        None
    }
}
