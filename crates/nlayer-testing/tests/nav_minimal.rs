use nlayer_bundler::compile_module;
use nlayer_core::geometry::{Points, Size};
use nlayer_testing::Headless;
use std::time::Duration;

#[test]
fn nav_stack_push_shows_detail() {
    let app_code = r#"
        import { mount } from "@nlayer/runtime";
        import { View, Text, NavStack, NavPage, createNavStack } from "@nlayer/ui";
        
        const nav = createNavStack([{ name: "home" }]);
        
        function App() {
            return (
                <NavStack state={nav}>
                    {(route) => {
                        if (route.name === "home") {
                            return (
                                <NavPage>
                                    <View testID="home" style={{ flex: 1 }}>
                                        <Text testID="title">Home</Text>
                                        <Text testID="open-detail" onPress={() => nav.push({ name: "detail" })}>
                                            Open Detail
                                        </Text>
                                    </View>
                                </NavPage>
                            );
                        } else {
                            return (
                                <NavPage>
                                    <View testID="detail" style={{ flex: 1 }}>
                                        <Text testID="detail-title">Detail</Text>
                                    </View>
                                </NavPage>
                            );
                        }
                    }}
                </NavStack>
            );
        }
        
        mount(App);
    "#;

    let bundle = compile_module("test.tsx", app_code).expect("compile");
    let mut app = Headless::start(bundle, Size::new(Points(390.0), Points(844.0))).expect("start");
    
    // Home should be visible initially
    assert!(app.wait_for("Home", Duration::from_secs(2)));
    assert!(app.screen().texts().contains(&"Home".to_string()));
    
    // Press to navigate
    app.press_selector("open-detail").expect("press");
    app.control().synchronize().expect("sync");
    app.refresh();
    
    // Detail should appear
    assert!(app.wait_selector("detail-title", Duration::from_secs(5)).is_ok(), "detail-title not found");
    let texts = app.texts();
    assert!(texts.contains(&"Detail".to_string()), "Detail text not found in: {:?}", texts);
    
    // Back should work
    app.back().expect("back");
    assert!(app.wait_for("Home", Duration::from_secs(2)));
    assert!(app.screen().texts().contains(&"Home".to_string()));
}
