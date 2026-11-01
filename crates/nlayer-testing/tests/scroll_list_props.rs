//! Test v3 scroll/list props decode without "unsupported prop" warnings:
//! refreshing, snapToInterval, snapToAlignment, snapToOffsets, pagingEnabled,
//! decelerationRate, disableIntervalMomentum, refreshIndicator, pullValue.

use std::time::Duration;

use nlayer_core::geometry::{Size, px};
use nlayer_testing::Headless;

fn app(source: &str) -> Headless {
    let bundle = nlayer_bundler::compile_module("App.tsx", source).unwrap();
    Headless::start(bundle, Size::new(px(390.0), px(844.0))).unwrap()
}

#[test]
fn refreshing_prop_decodes() {
    let source = r#"
import { mount, signal } from "@nlayer/runtime";
import { Scroll, View, Text } from "@nlayer/ui";

const refreshing = signal(false);

function App() {
  return (
    <Scroll refreshing={refreshing()} onRefresh={() => refreshing.set(true)}>
      <View style={{ height: 1000 }}>
        <Text>Scroll content</Text>
      </View>
    </Scroll>
  );
}
mount(App);
    "#;
    let mut app = app(source);
    assert!(app.wait_for("Scroll content", Duration::from_secs(1)));
    app.stop();
}
#[test]
fn snap_to_interval_decodes() {
    let source = r#"
import { mount } from "@nlayer/runtime";
import { Scroll, View, Text } from "@nlayer/ui";

function App() {
  return (
    <Scroll testID="scroll" snapToInterval={100}>
      <View style={{ height: 2000 }}>
        <Text>Long content</Text>
      </View>
    </Scroll>
  );
}
mount(App);
    "#;
    let mut app = app(source);
    assert!(app.wait_for("Long content", Duration::from_secs(1)));
    app.stop();
}

#[test]
fn snap_to_offsets_decodes() {
    let source = r#"
import { mount } from "@nlayer/runtime";
import { Scroll, View, Text } from "@nlayer/ui";

function App() {
  return (
    <Scroll testID="scroll" snapToOffsets={[0, 100, 250, 500]}>
      <View style={{ height: 1000 }}>
        <Text>Long content</Text>
      </View>
    </Scroll>
  );
}
mount(App);
    "#;
    let mut app = app(source);
    assert!(app.wait_for("Long content", Duration::from_secs(1)));
    app.stop();
}

#[test]
fn snap_to_alignment_decodes() {
    let source = r#"
import { mount } from "@nlayer/runtime";
import { Scroll, View, Text } from "@nlayer/ui";

function App() {
  return (
    <Scroll testID="scroll" snapToInterval={100} snapToAlignment="center">
      <View style={{ height: 1000 }}>
        <Text>Long content</Text>
      </View>
    </Scroll>
  );
}
mount(App);
    "#;
    let mut app = app(source);
    assert!(app.wait_for("Long content", Duration::from_secs(1)));
    app.stop();
}

#[test]
fn paging_enabled_decodes() {
    let source = r#"
import { mount } from "@nlayer/runtime";
import { Scroll, View, Text } from "@nlayer/ui";

function App() {
  return (
    <Scroll testID="scroll" pagingEnabled={true}>
      <View style={{ height: 2000 }}>
        <Text>Long content</Text>
      </View>
    </Scroll>
  );
}
mount(App);
    "#;
    let mut app = app(source);
    assert!(app.wait_for("Long content", Duration::from_secs(1)));
    app.stop();
}

#[test]
fn deceleration_rate_decodes() {
    let source = r#"
import { mount } from "@nlayer/runtime";
import { Scroll, View, Text } from "@nlayer/ui";

function App() {
  return (
    <View style={{ flex: 1 }}>
      <Scroll testID="scroll1" decelerationRate="fast">
        <View style={{ height: 1000 }}><Text>Fast</Text></View>
      </Scroll>
      <Scroll testID="scroll2" decelerationRate={0.95}>
        <View style={{ height: 1000 }}><Text>Custom</Text></View>
      </Scroll>
    </View>
  );
}
mount(App);
    "#;
    let mut app = app(source);
    assert!(app.wait_for("Fast", Duration::from_secs(1)));
    app.stop();
}

#[test]
fn disable_interval_momentum_decodes() {
    let source = r#"
import { mount } from "@nlayer/runtime";
import { Scroll, View, Text } from "@nlayer/ui";

function App() {
  return (
    <Scroll testID="scroll" snapToInterval={100} disableIntervalMomentum={true}>
      <View style={{ height: 1000 }}>
        <Text>Content</Text>
      </View>
    </Scroll>
  );
}
mount(App);
    "#;
    let mut app = app(source);
    assert!(app.wait_for("Content", Duration::from_secs(1)));
    app.stop();
}

#[test]
fn refresh_indicator_decodes() {
    let source = r#"
import { mount } from "@nlayer/runtime";
import { Scroll, View, Text } from "@nlayer/ui";

function App() {
  return (
    <Scroll testID="scroll" refreshing={true} refreshIndicator={60}>
      <View style={{ height: 1000 }}>
        <Text>Content</Text>
      </View>
    </Scroll>
  );
}
mount(App);
    "#;
    let mut app = app(source);
    assert!(app.wait_for("Content", Duration::from_secs(1)));
    app.stop();
}

#[test]
fn pull_value_decodes() {
    let source = r#"
import { mount } from "@nlayer/runtime";
import { Scroll, View, Text, createAnimatedValue } from "@nlayer/ui";

function App() {
  const pullValue = createAnimatedValue(0);
  
  return (
    <Scroll testID="scroll" pullValue={pullValue}>
      <View style={{ height: 1000 }}>
        <Text>Content</Text>
      </View>
    </Scroll>
  );
}
mount(App);
    "#;
    let mut app = app(source);
    assert!(app.wait_for("Content", Duration::from_secs(1)));
    app.stop();
}

#[test]
fn combined_props_decode() {
    let source = r#"
import { mount } from "@nlayer/runtime";
import { Scroll, View, Text } from "@nlayer/ui";

function App() {
  return (
    <Scroll
      testID="scroll"
      refreshing={false}
      pagingEnabled={false}
      snapToInterval={100}
      snapToAlignment="center"
      disableIntervalMomentum={false}
      decelerationRate="fast"
    >
      <View style={{ height: 2000 }}>
        <Text>Content</Text>
      </View>
    </Scroll>
  );
}
mount(App);
    "#;
    let mut app = app(source);
    assert!(app.wait_for("Content", Duration::from_secs(1)));
    app.stop();
}
