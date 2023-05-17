pub mod keybind;

use x11rb::connection::RequestConnection;
use x11rb::protocol::xkb::{self, ConnectionExt as _, StateNotifyEvent};
use x11rb::protocol::xproto::{ConnectionExt, GrabMode, ModMask, Screen};
use xcb::x::{Keysym, GRAB_ANY};
use xkbcommon::xkb::State as KBState;
use xkbcommon::xkb::{self as xkbc, KEY_Num_Lock};

use crate::config::commands;

use self::keybind::WKeybind;

pub struct WKeyboard {
    state: KBState,
    pub device_id: i32,
    pub keybinds: Vec<WKeybind>,
}

impl WKeyboard {
    pub fn new<'a, RC: RequestConnection>(
        conn: &'a RC,
        xcb_conn: &'a xcb::Connection,
        screen: &Screen,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        conn.prefetch_extension_information(xkb::X11_EXTENSION_NAME)?;

        let xkb = conn.xkb_use_extension(1, 0)?.reply()?;
        assert!(xkb.supported);

        let events = xkb::EventType::NEW_KEYBOARD_NOTIFY
            | xkb::EventType::MAP_NOTIFY
            | xkb::EventType::STATE_NOTIFY;
        let map_parts = xkb::MapPart::KEY_TYPES
            | xkb::MapPart::KEY_SYMS
            | xkb::MapPart::MODIFIER_MAP
            | xkb::MapPart::EXPLICIT_COMPONENTS
            | xkb::MapPart::KEY_ACTIONS
            | xkb::MapPart::KEY_BEHAVIORS
            | xkb::MapPart::VIRTUAL_MODS
            | xkb::MapPart::VIRTUAL_MOD_MAP;

        conn.xkb_select_events(
            xkb::ID::USE_CORE_KBD.into(),
            0u8.into(),
            events,
            map_parts,
            map_parts,
            &xkb::SelectEventsAux::new(),
        )?;

        let context = xkbc::Context::new(xkbc::CONTEXT_NO_FLAGS);
        let device_id = xkbc::x11::get_core_keyboard_device_id(xcb_conn);
        let keymap = xkbc::x11::keymap_new_from_device(
            &context,
            &xcb_conn,
            device_id,
            xkbc::KEYMAP_COMPILE_NO_FLAGS,
        );

        let state = xkbc::x11::state_new_from_device(&keymap, &xcb_conn, device_id);

        // grab all keybinds
        let keybinds = commands::setup_keybinds();

        let numlockmask = {
            let mut nlm: u16 = 0;
            let modmap = conn.get_modifier_mapping()?.reply()?;
            let max_keypermod = modmap.keycodes_per_modifier();
            for i in 0..8 {
                for j in 0..max_keypermod {
                    let idx = (i * max_keypermod + j) as usize;
                    if modmap.keycodes[idx] == KEY_Num_Lock as u8 {
                        nlm = 1 << i;
                    }
                }
            }
            ModMask::from(nlm)
        };
        let modifiers = [
            ModMask::from(0u16),
            ModMask::LOCK,
            numlockmask,
            numlockmask | ModMask::LOCK,
        ];

        conn.ungrab_key(GRAB_ANY, screen.root, ModMask::ANY)?;

        let (start, end) = (keymap.min_keycode(), keymap.max_keycode());

        for k in start..end {
            let syms = state.key_get_syms(k);

            if syms.is_empty() {
                continue;
            }

            for keybind in &keybinds {
                if syms.contains(&keybind.keysym) {
                    for m in &modifiers {
                        conn.grab_key(
                            true,
                            screen.root,
                            keybind.mods | *m,
                            k as u8,
                            GrabMode::ASYNC,
                            GrabMode::ASYNC,
                        )?;
                    }
                }
            }
        }

        Ok(Self {
            state,
            device_id,
            keybinds,
        })
    }

    pub fn update_state_mask(&mut self, evt: StateNotifyEvent) {
        self.state.update_mask(
            evt.base_mods.into(),
            evt.latched_mods.into(),
            evt.locked_mods.into(),
            evt.base_group.try_into().unwrap(),
            evt.latched_group.try_into().unwrap(),
            evt.locked_group.into(),
        );
    }

    pub fn key_sym(&self, detail: u32) -> Keysym {
        // we adjust for shift level here
        let level = self
            .state
            .key_get_level(detail, self.state.key_get_layout(detail));
        // FIXME: is this valid?
        self.state.key_get_one_sym(detail) + (level * 32)
    }
}
