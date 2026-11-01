use nlayer_bundler::compile_module;
use nlayer_core::geometry::{Points, Size};
use nlayer_testing::{Back, Headless};
use std::time::Duration;

#[test]
fn nav_page_pop_on_back_reactive() {
    let source = r#"
        import { signal, mount } from "@nlayer/runtime";
        import { NavPage, NavStack, Text, createNavStack } from "@nlayer/ui";

        const interceptBack = signal(false);
        const backIntercepted = signal(false);
        const nav = createNavStack([{ name: "home" }, { name: "detail" }]);

        function App() {
            return (
                <NavStack state={nav} style={{ flex: 1 }}>
                    {(route) => {
                        if (route.name === "detail") {
                            return (
                                <NavPage 
                                    popOnBack={!interceptBack()}
                                    onBackPress={() => {
                                        if (interceptBack()) {
                                            backIntercepted.set(true);
                                        }
                                    }}
                                >
                                    <Text testID="detail">Detail</Text>
                                    <Text testID="toggle" onPress={() => interceptBack.set(true)}>
                                        Toggle
                                    </Text>
                                    {backIntercepted() && <Text testID="intercepted">Intercepted</Text>}
                                </NavPage>
                            );
                        }
                        return <NavPage><Text>Home</Text></NavPage>;
                    }}
                </NavStack>
            );
        }
        mount(App);
    "#;

    let code = compile_module("test.tsx", source).expect("compile");
    let mut app = Headless::start(&code, Size::new(Points(390.0), Points(844.0))).expect("start");

    // Detail page is already mounted (nav starts with both routes)
    assert!(app.screen().texts().contains(&"Detail".to_string()));

    // Initially popOnBack is true (interceptBack is false)
    // With onBackPress registered, should return Ask
    assert!(
        matches!(app.screen().back_target(), Back::Ask(_)),
        "Expected Back::Ask initially, got {:?}",
        app.screen().back_target()
    );

    // Toggle interception by pressing button
    app.press_selector("toggle").expect("press toggle");
    app.control().synchronize().expect("sync");
    let _ = app.next_frame(Duration::from_millis(100));

    // Now popOnBack is false (interceptBack is true), still Ask since onBackPress is registered
    assert!(
        matches!(app.screen().back_target(), Back::Ask(_)),
        "Expected Back::Ask after toggling, got {:?}",
        app.screen().back_target()
    );

    // Actually call back() and verify the handler was called
    app.back().expect("back");
    assert!(
        app.screen().texts().contains(&"Intercepted".to_string()),
        "Expected 'Intercepted' text after back(), texts: {:?}",
        app.screen().texts()
    );
}
