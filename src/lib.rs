#![no_std]
use asr::{
    timer, timer::TimerState, watcher::Watcher, sync::Mutex
};
use asr_emu_help::genesis;

#[cfg(all(not(test), target_arch = "wasm32"))]
#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    core::arch::wasm32::unreachable()
}

static AUTOSPLITTER: Mutex<State> = Mutex::new(State {
    settings: None,
    watchers: Watchers {
        levelid: Watcher::new(),
        state: Watcher::new(),
        menu_timeout: Watcher::new(),
        menu_trigger: Watcher::new(),
        menu_selection: Watcher::new(),
    },
});

struct State {
    settings: Option<Settings>,
    watchers: Watchers,
}

struct Watchers {
    levelid: Watcher<Levels>,
    state: Watcher<u8>,
    menu_timeout: Watcher<u16>,
    menu_trigger: Watcher<u8>,
    menu_selection: Watcher<u8>,
}

#[derive(asr::Settings)]
struct Settings {
    #[default = true]
    /// START: Auto start
    start: bool,
    #[default = true]
    /// RESET: Auto reset
    reset: bool,
    #[default = true]
    /// Toxic Caves
    toxic_caves: bool,
    #[default = true]
    /// Bonus 1
    bonus_1: bool,
    #[default = true]
    /// Lava Powerhouse
    lava_powerhouse: bool,
    #[default = true]
    /// Bonus 2
    bonus_2: bool,
    #[default = true]
    /// The Machine
    the_machine: bool,
    #[default = true]
    /// Bonus 3
    bonus_3: bool,
    #[default = true]
    /// The Showdown
    the_showdown: bool,
}

impl State {
    fn update(&mut self) {
        let state = self.watchers.state.update_infallible(genesis::read::<u8>(0x3CB7).unwrap_or_default());

        // Determine the current level
        let level_1 = genesis::read::<u8>(0x5789).unwrap_or_default();
        let level_2 = genesis::read::<u8>(0x3CA9).unwrap_or_default();
        let current_level = match level_1 {
            // 0 => Levels::ToxicCaves,
            1 => match level_2 {
                1 | 2 => if state.current == 6 { Levels::Bonus1 } else { Levels::LavaPowerHouse },
                _ => Levels::LavaPowerHouse,
            },
            2 => match level_2 {
                1 | 2 => if state.current == 6 { Levels::Bonus2 } else { Levels::TheMachine },
                _ => Levels::TheMachine,
            },
            3 => match level_2 {
                1 | 2 => if state.current == 6 { Levels::Bonus3 } else { Levels::TheShowdown },
                _ => Levels::TheShowdown,
            },
            _ => Levels::ToxicCaves,
        };

        self.watchers.levelid.update_infallible(current_level);

        // Stuff related to the start trigger
        self.watchers.menu_timeout.update(genesis::read::<u16>(0xFF6C).ok());
        self.watchers.menu_trigger.update(genesis::read::<u8>(0xF2FC).ok());
        
        let temp_menu_selection = genesis::read::<u8>(0xFF69).unwrap_or_default();
        let menu_selection = match temp_menu_selection {
            1 | 2 | 15 => temp_menu_selection,
            _ => match &self.watchers.menu_selection.pair { Some(x) => x.current, _ => 0 },
        };
        self.watchers.menu_selection.update_infallible(menu_selection);
    }

    fn start(&self) -> bool {
        let Some(settings) = &self.settings else { return false };
        if !settings.start { return false };

        let Some(menu_timeout) = &self.watchers.menu_timeout.pair else { return false };
        let Some(menu_selection) = &self.watchers.menu_selection.pair else { return false };
        let Some(menu_trigger) = &self.watchers.menu_trigger.pair else { return false };
        let Some(state) = &self.watchers.state.pair else { return false };

        (menu_selection.old == 15 || menu_selection.old == 1) && state.current == 0 && menu_timeout.old > 10 && menu_trigger.old == 3 && menu_trigger.current < 3
    }

    fn split(&self) -> bool {
        let Some(settings) = &self.settings else { return false };
        let Some(level) = &self.watchers.levelid.pair else { return false };
        let Some(state) = &self.watchers.state.pair else { return false };
        
        match level.old {
            Levels::ToxicCaves => settings.toxic_caves && level.current == Levels::Bonus1,
            Levels::Bonus1 => settings.bonus_1 && level.current == Levels::LavaPowerHouse,
            Levels::LavaPowerHouse => settings.lava_powerhouse && level.current == Levels::Bonus2,
            Levels::Bonus2 => settings.bonus_2 && level.current == Levels::TheMachine,
            Levels::TheMachine => settings.the_machine && level.current == Levels::Bonus3,
            Levels::Bonus3 => settings.bonus_3 && level.current == Levels::TheShowdown,
            Levels::TheShowdown => settings.the_showdown && state.old == 2 && state.current == 4,
        }
    }

    fn reset(&self) -> bool {
        let Some(settings) = &self.settings else { return false };
        if !settings.reset { return false }
        let Some(state) = &self.watchers.state.pair else { return false };
        
        state.old > 0 && state.old <= 6 && state.current == 0
    }

/*
    fn is_loading(&self) -> Option<bool> {
        None
    }

    fn game_time(&self) -> Option<Duration> {
        None
    }
*/
}

#[no_mangle]
pub extern "C" fn update() {
    // Get access to the spinlock
    let autosplitter = &mut AUTOSPLITTER.lock();
    
    // Sets up the settings
    autosplitter.settings.get_or_insert_with(Settings::register);

    // Main autosplitter logic, essentially refactored from the OG LiveSplit autosplitting component.
    // First of all, the autosplitter needs to check if we managed to attach to the target process,
    // otherwise there's no need to proceed further.
    if !genesis::update() {
        return
    }

    // The main update logic is launched with this
    autosplitter.update();

    // Splitting logic. Adapted from OG LiveSplit:
    // Order of execution
    // 1. update() [this is launched above] will always be run first. There are no conditions on the execution of this action.
    // 2. If the timer is currently either running or paused, then the isLoading, gameTime, and reset actions will be run.
    // 3. If reset does not return true, then the split action will be run.
    // 4. If the timer is currently not running (and not paused), then the start action will be run.
    if timer::state() == TimerState::Running || timer::state() == TimerState::Paused {
/*
        if let Some(is_loading) = autosplitter.is_loading() {
            if is_loading {
                timer::pause_game_time()
            } else {
                timer::resume_game_time()
            }
        }

        if let Some(game_time) = autosplitter.game_time() {
            timer::set_game_time(game_time)
        }
*/
        if autosplitter.reset() {
            timer::reset()
        } else if autosplitter.split() {
            timer::split()
        }
    } 

    if timer::state() == TimerState::NotRunning {
        if autosplitter.start() {
            timer::start();
/*
            if let Some(is_loading) = autosplitter.is_loading() {
                if is_loading {
                    timer::pause_game_time()
                } else {
                    timer::resume_game_time()
                }
            }
*/
        }
    }     
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum Levels {
    ToxicCaves,
    Bonus1,
    LavaPowerHouse,
    Bonus2,
    TheMachine,
    Bonus3,
    TheShowdown,
}