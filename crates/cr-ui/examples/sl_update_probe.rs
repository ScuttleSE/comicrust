//! Isolated: new_smart_list → update_smart_list round trip.
fn main() {
    cr_ui::library::initialize().expect("session");
    let id = cr_ui::library::new_smart_list(None, "Probe List", "").expect("insert");
    println!("inserted: {id}");
    let mut item = cr_ui::library::find_smart_list(&id).expect("found");
    item.base.name = Some("Renamed".into());
    let changed = cr_ui::library::update_smart_list(&id, item);
    println!("update changed={changed}");
    let after = cr_ui::library::find_smart_list(&id);
    println!("after: {:?}", after.map(|i| i.base.name));
    let lists = cr_ui::library::comic_lists_snapshot();
    for l in &lists {
        println!("root item: {:?} {:?}", l.base().id, l.base().name);
    }
}
