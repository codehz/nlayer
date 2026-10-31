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
