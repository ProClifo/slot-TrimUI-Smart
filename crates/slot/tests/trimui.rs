//! What the TrimUI Smart's build leaves out: it has one core to spare and no L2 or R2.
#![cfg(feature = "trimui")]

mod common;

use common::{boot, tmp_root_with_carts};
use slot_ui::QuickRow;

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
