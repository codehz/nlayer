use nlayer_bundler::compile_module;
use nlayer_core::geometry::{Points, Size};
use nlayer_testing::Headless;
use std::time::Duration;

#[test]
fn pan_events_fire_on_view() {
    let source = r###"
        import { signal, mount } from "@nlayer/runtime";
        import { View, Text, createAnimatedValue } from "@nlayer/ui";

        const panStartFired = signal(false);
        const panEndX = signal(0);
        const translateX = createAnimatedValue(0);
        const translateY = createAnimatedValue(0);

        function App() {
            return (
                <View style={{ flex: 1 }}>
                    <Text testID="pan-start">{panStartFired() ? "started" : "no"}</Text>
                    <Text testID="pan-end">x={panEndX()}</Text>
                    <View
                        testID="card"
                        pan={{ x: translateX, y: translateY }}
                        onPanStart={(e) => panStartFired.set(true)}
                        onPanEnd={(e) => panEndX.set(Math.round(e.translation.x))}
                        style={{ width: 200, height: 200, backgroundColor: "red" }}
                    >
                        <Text>Drag</Text>
                    </View>
                </View>
            );
        }
        mount(App);
    "###;

    let code = compile_module("test.tsx", source).expect("compile");
    let mut app = Headless::start(&code, Size::new(Points(390.0), Points(844.0)))
        .expect("start");

    app.refresh();

    // Check initial state - should show "no" and "x=0"
    let initial_texts = app.texts();
    assert!(initial_texts.iter().any(|t| t.contains("no")), "Initial: {:?}", initial_texts);
    assert!(initial_texts.iter().any(|t| t.contains("x=0")), "Initial: {:?}", initial_texts);
    
    // Drag the card
    app.drag("card", 80.0, 0.0).expect("drag");
    app.control().synchronize().expect("sync after drag");
    app.refresh();
    
    // Check if PanStart fired
    let after_texts = app.texts();
    assert!(after_texts.iter().any(|t| t.contains("started")),
        "PanStart should fire. Texts: {:?}", after_texts);
    
    // Check if PanEnd fired with x≈80
    assert!(after_texts.iter().any(|t| t.contains("x=80") || t.contains("x=79") || t.contains("x=81")),
        "PanEnd should fire with x≈80. Texts: {:?}", after_texts);
}
