//! What the TrimUI Smart's build leaves out: it has one core to spare and no L2 or R2.
#![cfg(feature = "trimui")]

mod common;

use std::collections::VecDeque;

use common::{boot, clocked, tmp_root_with_carts};
use slot::app::Phase;
use slot::frontend::Frontend;
use slot::handoff::Handoff;
use slot_input::{Btn, InputSource, Millis, RawEvent};
use slot_power::SimPlatform;
use slot_store::Platform;
use slot_ui::QuickRow;

struct Script(VecDeque<Vec<RawEvent>>);

impl InputSource for Script {
    fn poll(&mut self, _now: Millis) -> Vec<RawEvent> {
        self.0.pop_front().unwrap_or_default()
    }
}

#[test]
fn rewind_is_never_recorded_even_when_the_card_asks_for_it() {
    let d = tmp_root_with_carts(&["Emerald"]);
    let mut state = slot_store::read_slot_state(d.path());
    state.rewind = true;
    slot_store::write_slot_state(d.path(), &state).unwrap();
    let app = boot(d.path());
    assert!(!app.records_rewind());
}

#[test]
fn neither_menu_offers_rewind() {
    let d = tmp_root_with_carts(&["Emerald"]);
    let app = boot(d.path());
    for hidden in [app.hidden_rows(), app.in_game_hidden()] {
        assert!(hidden.contains(&QuickRow::Rewind), "{hidden:?}");
        assert!(hidden.contains(&QuickRow::ScRewind), "{hidden:?}");
    }
}

#[test]
fn a_cart_that_goes_in_is_named_for_minuis_emulator_instead_of_played() {
    let d = tmp_root_with_carts(&["Emerald"]);
    clocked(d.path());
    let next = d.path().join("slot-next");
    std::env::set_var("SLOT_HANDOFF", &next);
    let mut f = Frontend::boot(Box::new(SimPlatform::at(d.path().to_path_buf())));
    let mut steps: VecDeque<Vec<RawEvent>> = (0..20).map(|_| Vec::new()).collect();
    steps.push_back(vec![RawEvent::Down(Btn::A)]);
    steps.push_back(vec![RawEvent::Up(Btn::A)]);
    let mut input = Script(steps);
    let mut now: Millis = 0;
    let mut frames = 0;
    while !f.handing_off() && frames < 600 {
        now += 17;
        frames += 1;
        f.advance_at(&mut input, now, 0.017);
    }
    assert!(f.handing_off(), "no handoff after {frames} frames");
    assert!(matches!(f.app().phase(), Phase::Inserting { .. }));
    f.hand_off().unwrap();
    let rom = d.path().join("Games/GBA/Emerald.gba");
    assert_eq!(
        std::fs::read_to_string(&next).unwrap(),
        format!("GBA\n{}\n", rom.display())
    );
}

#[test]
fn the_handoff_names_minuis_pak_for_each_platform() {
    for (platform, pak) in [
        (Platform::Gba, "GBA"),
        (Platform::Gb, "GB"),
        (Platform::Gbc, "GBC"),
    ] {
        let h = Handoff {
            platform,
            rom: "/mnt/SDCARD/slot/Games/x".into(),
        };
        assert_eq!(h.text(), format!("{pak}\n/mnt/SDCARD/slot/Games/x\n"));
    }
}

#[test]
fn a_cart_left_in_opens_on_the_shelf_for_the_boot_script_to_resume() {
    let d = tmp_root_with_carts(&["Emerald", "Zzz"]);
    let mut state = slot_store::read_slot_state(d.path());
    state.cart = Some("Emerald".into());
    state.cart_platform = Some(Platform::Gba);
    slot_store::write_slot_state(d.path(), &state).unwrap();
    let app = boot(d.path());
    assert!(matches!(app.phase(), Phase::Shelf), "{:?}", app.phase());
    assert!(app.seated_cart().is_none());
}
