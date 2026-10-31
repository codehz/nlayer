//! Tests for navigation: `createNavStack`, route mutations, and callback semantics.

use nlayer_core::{EventKind, HeaderSlot, Op, Prop, ScreenProp, Tag};
use nlayer_js::{EventPayload, Host};

fn create_id(ops: &[Op], tag: Tag) -> Option<u32> {
    ops.iter().find_map(|op| match op {
        Op::Create { id, tag: created } if *created == tag => Some(*id),
        _ => None,
    })
}

#[test]
fn create_nav_stack_with_initial_routes() {
    let host = Host::new().unwrap();
    host.eval_source(
        r#"
        import { createNavStack } from "@nlayer/ui";
        const nav = createNavStack([{ id: 1 }, { id: 2 }]);
        const result = JSON.stringify(nav.routes());
        if (result !== '[{"id":1},{"id":2}]') throw new Error(result);
        "#,
    )
    .unwrap();
}

#[test]
fn push_adds_route() {
    let host = Host::new().unwrap();
    host.eval_source(
        r#"
        import { createNavStack } from "@nlayer/ui";
        const nav = createNavStack([{ id: 1 }]);
        nav.push({ id: 2 });
        const result = JSON.stringify(nav.routes());
        if (result !== '[{"id":1},{"id":2}]') throw new Error(result);
        "#,
    )
    .unwrap();
}

#[test]
fn pop_removes_top_route() {
    let host = Host::new().unwrap();
    host.eval_source(
        r#"
        import { createNavStack } from "@nlayer/ui";
        const nav = createNavStack([{ id: 1 }, { id: 2 }, { id: 3 }]);
        nav.pop();
        const result = JSON.stringify(nav.routes());
        if (result !== '[{"id":1},{"id":2}]') throw new Error(result);
        "#,
    )
    .unwrap();
}

#[test]
fn pop_with_count() {
    let host = Host::new().unwrap();
    host.eval_source(
        r#"
        import { createNavStack } from "@nlayer/ui";
        const nav = createNavStack([{ id: 1 }, { id: 2 }, { id: 3 }, { id: 4 }]);
        nav.pop(2);
        const result = JSON.stringify(nav.routes());
        if (result !== '[{"id":1},{"id":2}]') throw new Error(result);
        "#,
    )
    .unwrap();
}

#[test]
fn pop_keeps_at_least_one_route() {
    let host = Host::new().unwrap();
    host.eval_source(
        r#"
        import { createNavStack } from "@nlayer/ui";
        const nav = createNavStack([{ id: 1 }]);
        nav.pop();
        const result = JSON.stringify(nav.routes());
        if (result !== '[{"id":1}]') throw new Error(result);
        "#,
    )
    .unwrap();
}

#[test]
fn pop_to_root() {
    let host = Host::new().unwrap();
    host.eval_source(
        r#"
        import { createNavStack } from "@nlayer/ui";
        const nav = createNavStack([{ id: 1 }, { id: 2 }, { id: 3 }]);
        nav.popToRoot();
        const result = JSON.stringify(nav.routes());
        if (result !== '[{"id":1}]') throw new Error(result);
        "#,
    )
    .unwrap();
}

#[test]
fn replace_top_route() {
    let host = Host::new().unwrap();
    host.eval_source(
        r#"
        import { createNavStack } from "@nlayer/ui";
        const nav = createNavStack([{ id: 1 }, { id: 2 }]);
        nav.replace({ id: 3 });
        const result = JSON.stringify(nav.routes());
        if (result !== '[{"id":1},{"id":3}]') throw new Error(result);
        "#,
    )
    .unwrap();
}

#[test]
fn set_routes_replaces_all() {
    let host = Host::new().unwrap();
    host.eval_source(
        r#"
        import { createNavStack } from "@nlayer/ui";
        const nav = createNavStack([{ id: 1 }, { id: 2 }]);
        nav.setRoutes([{ id: 3 }, { id: 4 }, { id: 5 }]);
        const result = JSON.stringify(nav.routes());
        if (result !== '[{"id":3},{"id":4},{"id":5}]') throw new Error(result);
        "#,
    )
    .unwrap();
}

#[test]
fn remove_finds_last_occurrence() {
    let host = Host::new().unwrap();
    host.eval_source(
        r#"
        import { createNavStack } from "@nlayer/ui";
        const route = { id: 2 };
        const nav = createNavStack([{ id: 1 }, route, { id: 3 }, route]);
        nav.remove(route);
        const result = JSON.stringify(nav.routes().map(r => r.id));
        if (result !== '[1,2,3]') throw new Error(result);
        "#,
    )
    .unwrap();
}

#[test]
fn remove_keeps_at_least_one_route() {
    let host = Host::new().unwrap();
    host.eval_source(
        r#"
        import { createNavStack } from "@nlayer/ui";
        const route = { id: 1 };
        const nav = createNavStack([route]);
        nav.remove(route);
        const result = JSON.stringify(nav.routes());
        if (result !== '[{"id":1}]') throw new Error(result);
        "#,
    )
    .unwrap();
}

#[test]
fn same_route_can_be_pushed_multiple_times() {
    let host = Host::new().unwrap();
    host.eval_source(
        r#"
        import { createNavStack } from "@nlayer/ui";
        const route = { id: 1 };
        const nav = createNavStack([route]);
        nav.push(route);
        nav.push(route);
        if (nav.routes().length !== 3) throw new Error(`length: ${nav.routes().length}`);
        "#,
    )
    .unwrap();
}

#[test]
fn routes_is_reactive() {
    let host = Host::new().unwrap();
    host.eval_source(
        r#"
        import { createNavStack } from "@nlayer/ui";
        import { effect } from "@nlayer/runtime";
        const nav = createNavStack([{ id: 1 }]);
        const log = [];
        effect(() => {
            log.push(nav.routes().length);
        });
        nav.push({ id: 2 });
        nav.pop();
        const result = JSON.stringify(log);
        if (result !== '[1,2,1]') throw new Error(result);
        "#,
    )
    .unwrap();
}

#[test]
fn mutations_trigger_routes_accessor() {
    let host = Host::new().unwrap();
    host.eval_source(
        r#"
        import { createNavStack } from "@nlayer/ui";
        import { effect } from "@nlayer/runtime";
        const nav = createNavStack([{ id: 1 }]);
        let count = 0;
        effect(() => {
            nav.routes();
            count++;
        });
        nav.push({ id: 2 });
        nav.replace({ id: 3 });
        nav.popToRoot();
        if (count !== 4) throw new Error(`count: ${count}`);
        "#,
    )
    .unwrap();
}

#[test]
fn nav_stack_component_exists() {
    let host = Host::new().unwrap();
    host.eval_source(
        r#"
        import { NavStack } from "@nlayer/ui";
        if (typeof NavStack !== 'function') throw new Error(`NavStack: ${typeof NavStack}`);
        "#,
    )
    .unwrap();
}

#[test]
fn nav_page_component_exists() {
    let host = Host::new().unwrap();
    host.eval_source(
        r#"
        import { NavPage } from "@nlayer/ui";
        if (typeof NavPage !== 'function') throw new Error(`NavPage: ${typeof NavPage}`);
        "#,
    )
    .unwrap();
}

#[test]
fn dialog_component_exists() {
    let host = Host::new().unwrap();
    host.eval_source(
        r#"
        import { Dialog } from "@nlayer/ui";
        if (typeof Dialog !== 'function') throw new Error(`Dialog: ${typeof Dialog}`);
        "#,
    )
    .unwrap();
}

#[test]
fn nav_page_creates_screen_and_sets_title() {
    let host = Host::new().unwrap();
    host.eval_source(
        r#"
        import { NavPage } from "@nlayer/ui";
        import { mount } from "@nlayer/runtime";
        globalThis.dispose = mount(() => NavPage({ title: "My Title", children: "content" }));
        "#,
    )
    .unwrap();

    let mut ops = Vec::new();
    host.take_ops(&mut ops);
    assert!(create_id(&ops, Tag::Screen).is_some());
    assert!(ops.iter().any(|op| matches!(
        op,
        Op::SetProp {
            prop: Prop::Screen(ScreenProp::Title(Some(title))),
            ..
        } if title.as_ref() == "My Title"
    )));
}

#[test]
fn nav_page_dispatches_back_and_dismiss_callbacks() {
    let host = Host::new().unwrap();
    host.eval_source(
        r#"
        import { NavPage } from "@nlayer/ui";
        import { mount } from "@nlayer/runtime";
        globalThis.backCalls = "0";
        globalThis.dismissCalls = "0";
        globalThis.dispose = mount(() => NavPage({
          onBackPress: () => { backCalls = String(Number(backCalls) + 1); },
          onDismiss: () => { dismissCalls = String(Number(dismissCalls) + 1); },
          children: "content"
        }));
        "#,
    )
    .unwrap();
    let mut ops = Vec::new();
    host.take_ops(&mut ops);
    let id = create_id(&ops, Tag::Screen).expect("NavPage creates a screen");

    host.dispatch_event(id, EventKind::BackRequest, &EventPayload::None);
    host.dispatch_event(id, EventKind::Dismiss, &EventPayload::None);
    assert_eq!(host.global_string("backCalls").as_deref(), Some("1"));
    assert_eq!(host.global_string("dismissCalls").as_deref(), Some("1"));
}

#[test]
fn nav_page_creates_header_items_in_the_requested_slots() {
    let host = Host::new().unwrap();
    host.eval_source(
        r#"
        import { NavPage } from "@nlayer/ui";
        import { mount } from "@nlayer/runtime";
        globalThis.dispose = mount(() => NavPage({ headerLeft: "Left", headerRight: "Right", headerTitle: "Title", children: "content" }));
        "#,
    )
    .unwrap();

    let mut ops = Vec::new();
    host.take_ops(&mut ops);
    let slots: Vec<_> = ops
        .iter()
        .filter_map(|op| match op {
            Op::SetProp {
                prop: Prop::HeaderSlot(slot),
                ..
            } => Some(*slot),
            _ => None,
        })
        .collect();
    assert_eq!(
        slots,
        [HeaderSlot::Left, HeaderSlot::Right, HeaderSlot::Title]
    );
}

#[test]
fn dialog_mounts_and_reports_show_and_dismiss() {
    let host = Host::new().unwrap();
    host.eval_source(
        r#"
        import { Dialog } from "@nlayer/ui";
        import { mount } from "@nlayer/runtime";
        globalThis.showCalls = "0";
        globalThis.closeCalls = "0";
        globalThis.dispose = mount(() => Dialog({ open: true, onOpen: () => { showCalls = String(Number(showCalls) + 1); }, onClose: () => { closeCalls = String(Number(closeCalls) + 1); }, children: "content" }));
        "#,
    )
    .unwrap();
    let mut ops = Vec::new();
    host.take_ops(&mut ops);
    let id = create_id(&ops, Tag::Modal).expect("open Dialog creates a modal");

    host.dispatch_event(id, EventKind::Show, &EventPayload::None);
    host.dispatch_event(id, EventKind::Dismiss, &EventPayload::None);
    assert_eq!(host.global_string("showCalls").as_deref(), Some("1"));
    assert_eq!(host.global_string("closeCalls").as_deref(), Some("1"));
}

#[test]
fn closed_dialog_does_not_create_a_modal() {
    let host = Host::new().unwrap();
    host.eval_source(
        r#"
        import { Dialog } from "@nlayer/ui";
        import { mount } from "@nlayer/runtime";
        globalThis.dispose = mount(() => Dialog({ open: false, children: "content" }));
        "#,
    )
    .unwrap();
    let mut ops = Vec::new();
    host.take_ops(&mut ops);
    assert!(create_id(&ops, Tag::Modal).is_none());
}

#[test]
fn dialog_request_close_calls_cancel_and_reports_reason() {
    let host = Host::new().unwrap();
    host.eval_source(
        r#"
        import { Dialog } from "@nlayer/ui";
        import { mount } from "@nlayer/runtime";
        globalThis.cancelCalls = "0";
        globalThis.change = "";
        globalThis.dispose = mount(() => Dialog({
          open: true,
          onCancel: () => { cancelCalls = String(Number(cancelCalls) + 1); },
          onOpenChange: event => { change = `${event.open}:${event.reason}`; },
          children: "content"
        }));
        "#,
    )
    .unwrap();
    let mut ops = Vec::new();
    host.take_ops(&mut ops);
    let id = create_id(&ops, Tag::Modal).expect("open Dialog creates a modal");

    host.dispatch_event(id, EventKind::RequestClose, &EventPayload::None);
    assert_eq!(host.global_string("cancelCalls").as_deref(), Some("1"));
    assert_eq!(
        host.global_string("change").as_deref(),
        Some("false:cancel")
    );
}

#[test]
fn dialog_close_flag_blocks_open_change_but_still_calls_cancel() {
    let host = Host::new().unwrap();
    host.eval_source(
        r#"
        import { Dialog } from "@nlayer/ui";
        import { mount } from "@nlayer/runtime";
        globalThis.cancelCalls = "0";
        globalThis.changeCalls = "0";
        globalThis.dispose = mount(() => Dialog({
          open: true,
          closeOnEscape: false,
          onCancel: () => { cancelCalls = String(Number(cancelCalls) + 1); },
          onOpenChange: () => { changeCalls = String(Number(changeCalls) + 1); },
          children: "content"
        }));
        "#,
    )
    .unwrap();
    let mut ops = Vec::new();
    host.take_ops(&mut ops);
    let id = create_id(&ops, Tag::Modal).expect("open Dialog creates a modal");

    host.dispatch_event(id, EventKind::RequestClose, &EventPayload::None);
    assert_eq!(host.global_string("cancelCalls").as_deref(), Some("1"));
    assert_eq!(host.global_string("changeCalls").as_deref(), Some("0"));
}

#[test]
fn dialog_creates_backdrop_and_popup_views() {
    let host = Host::new().unwrap();
    host.eval_source(
        r#"
        import { Dialog } from "@nlayer/ui";
        import { mount } from "@nlayer/runtime";
        globalThis.dispose = mount(() => Dialog({ open: true, backdropStyle: { opacity: 0.5 }, children: "content" }));
        "#,
    )
    .unwrap();
    let mut ops = Vec::new();
    host.take_ops(&mut ops);
    assert!(create_id(&ops, Tag::Modal).is_some());
    assert!(
        ops.iter()
            .filter(|op| matches!(op, Op::Create { tag: Tag::View, .. }))
            .count()
            >= 2
    );
}

#[test]
fn nav_stack_renders_screens_for_each_route() {
    let host = Host::new().unwrap();
    host.eval_source(
        r#"
        import { createNavStack, NavStack, NavPage } from "@nlayer/ui";
        import { mount } from "@nlayer/runtime";
        const nav = createNavStack([{ id: 1 }, { id: 2 }]);
        globalThis.dispose = mount(() => 
            NavStack({ state: nav, children: (route, index) => 
                NavPage({ title: `Page ${route.id}`, children: `Content ${index}` })
            })
        );
        "#,
    )
    .unwrap();

    let mut ops = Vec::new();
    host.take_ops(&mut ops);

    // Should have created a Stack
    assert!(create_id(&ops, Tag::Stack).is_some());

    // Should have created 2 Screens (one per route)
    let screens: Vec<_> = ops
        .iter()
        .filter_map(|op| match op {
            Op::Create {
                id,
                tag: Tag::Screen,
            } => Some(*id),
            _ => None,
        })
        .collect();
    assert_eq!(screens.len(), 2, "Should create one Screen per route");
}

#[test]
fn nav_stack_passes_route_and_index_to_children() {
    let host = Host::new().unwrap();
    host.eval_source(
        r#"
        import { createNavStack, NavStack, NavPage } from "@nlayer/ui";
        import { mount } from "@nlayer/runtime";
        const nav = createNavStack([{ name: "A" }, { name: "B" }]);
        globalThis.calls = [];
        globalThis.dispose = mount(() => 
            NavStack({ state: nav, children: (route, index) => {
                calls.push({ route: route.name, index: index() });
                return NavPage({ children: route.name });
            }})
        );
        "#,
    )
    .unwrap();

    host.eval_source(r#"globalThis.callsJson = JSON.stringify(calls);"#)
        .unwrap();
    let calls = host.global_string("callsJson").unwrap();
    assert_eq!(
        calls,
        r#"[{"route":"A","index":0},{"route":"B","index":1}]"#
    );
}

#[test]
fn nav_stack_same_route_twice_renders_two_screens() {
    let host = Host::new().unwrap();
    host.eval_source(
        r#"
        import { createNavStack, NavStack, NavPage } from "@nlayer/ui";
        import { mount } from "@nlayer/runtime";
        const route = { id: 1 };
        const nav = createNavStack([route, route]);
        globalThis.dispose = mount(() => 
            NavStack({ state: nav, children: (r, i) => NavPage({ children: `${i}` }) })
        );
        "#,
    )
    .unwrap();

    let mut ops = Vec::new();
    host.take_ops(&mut ops);

    let screen_count = ops
        .iter()
        .filter(|op| {
            matches!(
                op,
                Op::Create {
                    tag: Tag::Screen,
                    ..
                }
            )
        })
        .count();
    assert_eq!(
        screen_count, 2,
        "Same route pushed twice should render two Screens"
    );
}

#[test]
fn nav_stack_push_updates_dom_rows() {
    let host = Host::new().unwrap();
    host.eval_source(
        r#"
        import { createNavStack, NavStack, NavPage } from "@nlayer/ui";
        import { mount } from "@nlayer/runtime";
        const nav = createNavStack([{ id: 1 }]);
        globalThis.nav = nav;
        globalThis.dispose = mount(() => 
            NavStack({ state: nav, children: (route) => NavPage({ children: `${route.id}` }) })
        );
        "#,
    )
    .unwrap();

    let mut ops = Vec::new();
    host.take_ops(&mut ops);
    let initial_screens = ops
        .iter()
        .filter(|op| {
            matches!(
                op,
                Op::Create {
                    tag: Tag::Screen,
                    ..
                }
            )
        })
        .count();
    assert_eq!(initial_screens, 1);

    // Push a new route
    host.eval_source(r#"nav.push({ id: 2 });"#).unwrap();
    ops.clear();
    host.take_ops(&mut ops);

    let new_screens = ops
        .iter()
        .filter(|op| {
            matches!(
                op,
                Op::Create {
                    tag: Tag::Screen,
                    ..
                }
            )
        })
        .count();
    assert_eq!(new_screens, 1, "Push should create one new Screen");
}

#[test]
fn nav_stack_pop_updates_dom_rows() {
    let host = Host::new().unwrap();
    host.eval_source(
        r#"
        import { createNavStack, NavStack, NavPage } from "@nlayer/ui";
        import { mount } from "@nlayer/runtime";
        const nav = createNavStack([{ id: 1 }, { id: 2 }]);
        globalThis.nav = nav;
        globalThis.dispose = mount(() => 
            NavStack({ state: nav, children: (route) => NavPage({ children: `${route.id}` }) })
        );
        "#,
    )
    .unwrap();

    let mut ops = Vec::new();
    host.take_ops(&mut ops);

    // Pop a route
    host.eval_source(r#"nav.pop();"#).unwrap();
    ops.clear();
    host.take_ops(&mut ops);

    let removes = ops
        .iter()
        .filter(|op| matches!(op, Op::Remove { .. }))
        .count();
    assert!(removes > 0, "Pop should remove the popped screen");
}

#[test]
fn nav_stack_set_routes_updates_dom_rows() {
    let host = Host::new().unwrap();
    host.eval_source(
        r#"
        import { createNavStack, NavStack, NavPage } from "@nlayer/ui";
        import { mount } from "@nlayer/runtime";
        const nav = createNavStack([{ id: 1 }]);
        globalThis.nav = nav;
        globalThis.dispose = mount(() => 
            NavStack({ state: nav, children: (route) => NavPage({ children: `${route.id}` }) })
        );
        "#,
    )
    .unwrap();

    let mut ops = Vec::new();
    host.take_ops(&mut ops);

    // Replace all routes
    host.eval_source(r#"nav.setRoutes([{ id: 2 }, { id: 3 }, { id: 4 }]);"#)
        .unwrap();
    ops.clear();
    host.take_ops(&mut ops);

    let new_screens = ops
        .iter()
        .filter(|op| {
            matches!(
                op,
                Op::Create {
                    tag: Tag::Screen,
                    ..
                }
            )
        })
        .count();
    assert_eq!(
        new_screens, 3,
        "setRoutes should create Screens for new routes"
    );
}

#[test]
fn dismiss_event_removes_route_then_calls_on_dismiss() {
    let host = Host::new().unwrap();
    host.eval_source(
        r#"
        import { createNavStack, NavStack, NavPage } from "@nlayer/ui";
        import { mount } from "@nlayer/runtime";
        const nav = createNavStack([{ id: 1 }, { id: 2 }]);
        globalThis.nav = nav;
        globalThis.order = [];
        globalThis.dispose = mount(() => 
            NavStack({ state: nav, children: (route, index) => 
                NavPage({ 
                    onDismiss: () => {
                        order.push("dismiss");
                        order.push(nav.routes().length);
                    },
                    children: `${route.id}` 
                })
            })
        );
        "#,
    )
    .unwrap();

    let mut ops = Vec::new();
    host.take_ops(&mut ops);

    // Get the second screen's ID
    let screens: Vec<_> = ops
        .iter()
        .filter_map(|op| match op {
            Op::Create {
                id,
                tag: Tag::Screen,
            } => Some(*id),
            _ => None,
        })
        .collect();
    assert_eq!(screens.len(), 2);
    let second_screen = screens[1];

    // Dispatch Dismiss event on the second screen
    host.dispatch_event(second_screen, EventKind::Dismiss, &EventPayload::None);

    // Check that the route was removed before onDismiss was called
    host.eval_source(r#"globalThis.orderJson = JSON.stringify(order);"#)
        .unwrap();
    let order = host.global_string("orderJson").unwrap();
    assert_eq!(
        order, r#"["dismiss",1]"#,
        "Route should be removed before onDismiss callback"
    );
}

#[test]
fn nav_page_header_slots_create_header_items_with_slot_prop() {
    let host = Host::new().unwrap();
    host.eval_source(
        r#"
        import { NavPage } from "@nlayer/ui";
        import { mount } from "@nlayer/runtime";
        globalThis.dispose = mount(() => 
            NavPage({ 
                headerLeft: "Left", 
                headerRight: "Right", 
                headerTitle: "Title",
                header: "Custom",
                children: "content" 
            })
        );
        "#,
    )
    .unwrap();

    let mut ops = Vec::new();
    host.take_ops(&mut ops);

    // Check that HeaderItem nodes were created with headerSlot props
    let header_items: Vec<_> = ops
        .iter()
        .filter_map(|op| match op {
            Op::Create {
                id,
                tag: Tag::HeaderItem,
            } => Some(*id),
            _ => None,
        })
        .collect();
    assert_eq!(
        header_items.len(),
        4,
        "Should create HeaderItem for each slot"
    );

    let slots: Vec<_> = ops
        .iter()
        .filter_map(|op| match op {
            Op::SetProp {
                prop: Prop::HeaderSlot(slot),
                ..
            } => Some(*slot),
            _ => None,
        })
        .collect();
    assert!(slots.contains(&HeaderSlot::Left));
    assert!(slots.contains(&HeaderSlot::Right));
    assert!(slots.contains(&HeaderSlot::Title));
    assert!(slots.contains(&HeaderSlot::Header));
}

#[test]
fn dialog_does_not_spread_on_insets_to_modal() {
    let host = Host::new().unwrap();
    host.eval_source(
        r#"
        import { Dialog } from "@nlayer/ui";
        import { mount } from "@nlayer/runtime";
        globalThis.insetsCalled = 0;
        globalThis.dispose = mount(() => 
            Dialog({ 
                open: true,
                onInsets: () => { insetsCalled = 1; },
                children: "content" 
            })
        );
        "#,
    )
    .unwrap();

    let mut ops = Vec::new();
    host.take_ops(&mut ops);

    // Check that Modal was created
    let _modal_id = create_id(&ops, Tag::Modal).expect("Dialog should create Modal");

    // onInsets should not have been called (it's not spread to modal)
    host.eval_source(r#"globalThis.callStatus = String(insetsCalled);"#)
        .unwrap();
    let called = host.global_string("callStatus").unwrap();
    assert_eq!(called, "0", "onInsets should not be spread to Modal");
}
