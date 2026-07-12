#![allow(linker_messages)] // MSVC emits a harmless import-library notice for the cdylib test build.

//! Browser entry point for the interactive iptools exhibit.

#[cfg(target_arch = "wasm32")]
mod wasm {
    use std::{cell::RefCell, rc::Rc, str::FromStr};

    use iptools_core::{
        Action, AppModel, ConfigData, Effect, InputEvent, KeyCode, KeyEvent, Language, Message,
        Modifiers, SessionState,
    };
    use iptools_demo::{DemoRuntime, ScenarioId};
    use iptools_ui::UiState;
    use ratzilla::{
        CanvasBackend, DomBackend, WebEventHandler, WebRenderer,
        backend::canvas::CanvasBackendOptions,
        event::{MouseButton, MouseEventKind},
        ratatui::{Terminal, backend::Backend},
    };
    use wasm_bindgen::{JsCast, JsValue, closure::Closure, prelude::wasm_bindgen};
    use web_sys::{Element, Event, KeyboardEvent, UrlSearchParams, WheelEvent, window};

    struct WebApp {
        scenario: ScenarioId,
        config: ConfigData,
        model: AppModel,
        runtime: DemoRuntime,
        ui: UiState,
        last_frame_ms: f64,
        last_clock_second: u64,
        input_generation: u64,
        state_revision: u64,
        scheduled_revision: Option<u64>,
    }

    impl WebApp {
        fn new(scenario: ScenarioId, config: ConfigData) -> Result<Self, JsValue> {
            let runtime = DemoRuntime::new(scenario).map_err(js_error)?;
            let mut model = AppModel::default();
            model.apply_config(&config);
            for event in runtime.bootstrap() {
                model.update(Message::Runtime(event));
            }
            Ok(Self {
                scenario,
                config,
                model,
                runtime,
                ui: UiState::default(),
                last_frame_ms: performance_now(),
                last_clock_second: 0,
                input_generation: 0,
                state_revision: 0,
                scheduled_revision: None,
            })
        }

        fn input(&mut self, input: InputEvent) {
            if matches!(input, InputEvent::Action(Action::ResetDemo)) {
                if let Err(error) = self.reset_demo() {
                    web_sys::console::warn_1(&error);
                }
                return;
            }
            self.input_generation = self.input_generation.saturating_add(1);
            self.state_revision = self.state_revision.saturating_add(1);
            let previous_language = self.model.language;
            let effects = self.model.update(Message::Input(input));
            self.dispatch(effects);
            if self.model.language != previous_language {
                if let Err(error) = replace_language_query(self.model.language) {
                    web_sys::console::warn_1(&error);
                }
            }
        }

        fn reset_demo(&mut self) -> Result<(), JsValue> {
            self.config.session = SessionState::default();
            persist_config(&self.config);
            self.runtime = DemoRuntime::new(self.scenario).map_err(js_error)?;
            self.model = AppModel::default();
            self.model.apply_config(&self.config);
            for event in self.runtime.bootstrap() {
                self.model.update(Message::Runtime(event));
            }
            self.ui = UiState::default();
            self.last_frame_ms = performance_now();
            self.last_clock_second = 0;
            self.input_generation = self.input_generation.saturating_add(1);
            self.state_revision = self.state_revision.saturating_add(1);
            Ok(())
        }

        fn tick(&mut self) {
            let now = performance_now();
            let delta = (now - self.last_frame_ms).clamp(0.0, 250.0) as u64;
            if delta < 16 {
                return;
            }
            self.last_frame_ms = now;
            let effects = self.model.update(Message::Tick(delta));
            self.dispatch(effects);
            let clock_second = (js_sys::Date::now() / 1_000.0) as u64;
            if clock_second != self.last_clock_second {
                self.last_clock_second = clock_second;
                self.state_revision = self.state_revision.saturating_add(1);
                self.model.update(Message::Clock(browser_clock()));
            }
            for event in self.runtime.advance(delta) {
                self.state_revision = self.state_revision.saturating_add(1);
                let effects = self.model.update(Message::Runtime(event));
                self.dispatch(effects);
            }
        }

        fn dispatch(&mut self, effects: Vec<Effect>) {
            for effect in effects {
                if self.config.apply_persistence_effect(&effect) {
                    persist_config(&self.config);
                    persist_legacy_preferences(&self.config);
                    continue;
                }
                for event in self.runtime.dispatch(effect) {
                    self.state_revision = self.state_revision.saturating_add(1);
                    self.model.update(Message::Runtime(event));
                }
            }
        }
    }

    fn initial_config(params: &UrlSearchParams) -> ConfigData {
        let mut config = load_config().unwrap_or_else(|| ConfigData {
            language: load_language().unwrap_or_else(browser_language),
            scan_concurrency: load_scan_concurrency().unwrap_or(50),
            ..ConfigData::default()
        });
        if let Some(language) = params.get("lang") {
            config.language = parse_language(&language);
        }
        config.scan_concurrency = config.scan_concurrency.clamp(10, 500);
        config
    }

    #[wasm_bindgen(start)]
    pub fn start() -> Result<(), JsValue> {
        console_error_panic_hook::set_once();
        let params = query_params()?;
        let scenario = params
            .get("scenario")
            .and_then(|value| ScenarioId::from_str(&value).ok())
            .or_else(load_scenario)
            .unwrap_or_default();
        let config = initial_config(&params);
        let language = config.language;
        persist_preferences(scenario, &config);

        let app = Rc::new(RefCell::new(WebApp::new(scenario, config)?));
        install_soft_keys(Rc::clone(&app))?;
        install_wheel(Rc::clone(&app))?;

        let renderer_from_url = params.get("renderer");
        let renderer = renderer_from_url.clone().or_else(load_renderer);
        let force_dom = match renderer.as_deref() {
            Some("dom") => true,
            Some("canvas") => false,
            _ => language == Language::Zh,
        };
        if !force_dom {
            let options = CanvasBackendOptions::new()
                .grid_id("terminal")
                .font("16px 'Maple Mono CN iptools', 'Maple Mono NF CN', monospace");
            match CanvasBackend::new_with_options(options) {
                Ok(backend) => {
                    persist_renderer("canvas");
                    return run_backend(backend, app);
                }
                Err(error) => web_sys::console::warn_1(
                    &format!("Canvas renderer unavailable, falling back to DOM: {error}").into(),
                ),
            }
        }
        persist_renderer("dom");
        run_backend(DomBackend::new_by_id("terminal").map_err(js_error)?, app)
    }

    fn run_backend<B>(backend: B, app: Rc<RefCell<WebApp>>) -> Result<(), JsValue>
    where
        B: Backend + WebEventHandler + 'static,
        B::Error: std::fmt::Display,
    {
        let mut terminal = Terminal::new(backend).map_err(js_error)?;
        install_keyboard(Rc::clone(&app))?;
        let mouse = Rc::clone(&app);
        terminal
            .on_mouse_event(move |event| {
                let action = match event.kind {
                    MouseEventKind::ButtonDown(MouseButton::Left) => {
                        mouse.borrow().ui.hit_test(event.col, event.row)
                    }
                    _ => None,
                };
                if let Some(action) = action {
                    mouse.borrow_mut().input(InputEvent::Action(action));
                }
            })
            .map_err(js_error)?;
        terminal.draw_web(move |frame| {
            let mut state = app.borrow_mut();
            state.tick();
            let input_generation = state.input_generation;
            let state_revision = state.state_revision;
            let should_mark = state.scheduled_revision != Some(state_revision);
            if should_mark {
                state.scheduled_revision = Some(state_revision);
            }
            let WebApp { model, ui, .. } = &mut *state;
            iptools_ui::render(frame, model, ui);
            if should_mark {
                mark_rendered(input_generation, state_revision);
            }
        });
        focus_terminal();
        Ok(())
    }

    fn install_keyboard(app: Rc<RefCell<WebApp>>) -> Result<(), JsValue> {
        let document = window()
            .and_then(|value| value.document())
            .ok_or("document unavailable")?;
        let closure = Closure::<dyn FnMut(KeyboardEvent)>::new(move |event: KeyboardEvent| {
            let inside_terminal = event
                .target()
                .and_then(|target| target.dyn_into::<Element>().ok())
                .and_then(|target| target.closest("#terminal").ok().flatten())
                .is_some();
            if inside_terminal {
                let key = event.key();
                let controlled_action = event.ctrl_key()
                    && matches!(key.to_ascii_lowercase().as_str(), "c" | "l" | "q" | "r");
                if controlled_action
                    || matches!(
                        key.as_str(),
                        "F1" | "Tab" | " " | "ArrowUp" | "ArrowDown" | "ArrowLeft" | "ArrowRight"
                    )
                {
                    event.prevent_default();
                }
                if let Some(key) = convert_key(&event) {
                    app.borrow_mut().input(InputEvent::Key(key));
                }
            }
        });
        document.add_event_listener_with_callback("keydown", closure.as_ref().unchecked_ref())?;
        closure.forget();
        Ok(())
    }

    fn install_soft_keys(app: Rc<RefCell<WebApp>>) -> Result<(), JsValue> {
        let document = window()
            .and_then(|value| value.document())
            .ok_or("document unavailable")?;
        let elements = document.query_selector_all("[data-action]")?;
        for index in 0..elements.length() {
            let Some(element) = elements.get(index) else {
                continue;
            };
            let element: Element = element.dyn_into()?;
            let Some(name) = element.get_attribute("data-action") else {
                continue;
            };
            let app = Rc::clone(&app);
            let closure = Closure::<dyn FnMut(Event)>::new(move |event: Event| {
                event.prevent_default();
                if let Some(action) = parse_action(&name) {
                    app.borrow_mut().input(InputEvent::Action(action));
                }
                focus_terminal();
            });
            element.add_event_listener_with_callback("click", closure.as_ref().unchecked_ref())?;
            closure.forget();
        }
        Ok(())
    }

    fn install_wheel(app: Rc<RefCell<WebApp>>) -> Result<(), JsValue> {
        let document = window()
            .and_then(|value| value.document())
            .ok_or("document unavailable")?;
        let terminal = document
            .get_element_by_id("terminal")
            .ok_or("terminal element unavailable")?;
        let closure = Closure::<dyn FnMut(WheelEvent)>::new(move |event: WheelEvent| {
            event.prevent_default();
            let action = if event.delta_y() < 0.0 {
                Action::Up
            } else {
                Action::Down
            };
            app.borrow_mut().input(InputEvent::Action(action));
        });
        terminal.add_event_listener_with_callback("wheel", closure.as_ref().unchecked_ref())?;
        closure.forget();
        Ok(())
    }

    fn convert_key(event: &KeyboardEvent) -> Option<KeyEvent> {
        let value = event.key();
        let code = match value.as_str() {
            "Enter" => KeyCode::Enter,
            "Escape" => KeyCode::Esc,
            "Tab" if event.shift_key() => KeyCode::BackTab,
            "Tab" => KeyCode::Tab,
            "Backspace" => KeyCode::Backspace,
            "Delete" => KeyCode::Delete,
            "Home" => KeyCode::Home,
            "End" => KeyCode::End,
            "ArrowUp" => KeyCode::Up,
            "ArrowDown" => KeyCode::Down,
            "ArrowLeft" => KeyCode::Left,
            "ArrowRight" => KeyCode::Right,
            value if value.len() == 1 => KeyCode::Char(value.chars().next()?),
            value if value.starts_with('F') => {
                KeyCode::F(value.trim_start_matches('F').parse().ok()?)
            }
            _ => return None,
        };
        Some(KeyEvent {
            code,
            modifiers: Modifiers {
                control: event.ctrl_key(),
                alt: event.alt_key(),
                shift: event.shift_key(),
            },
        })
    }

    fn parse_action(name: &str) -> Option<Action> {
        match name {
            "up" => Some(Action::Up),
            "down" => Some(Action::Down),
            "left" => Some(Action::Left),
            "right" => Some(Action::Right),
            "confirm" => Some(Action::Confirm),
            "back" => Some(Action::Back),
            "next" => Some(Action::NextPage),
            "previous" => Some(Action::PreviousPage),
            "toggle" => Some(Action::Toggle),
            "history" => Some(Action::History),
            "help" => Some(Action::Help),
            "language" => Some(Action::ToggleLanguage),
            "reset" => Some(Action::ResetDemo),
            _ => None,
        }
    }

    fn query_params() -> Result<UrlSearchParams, JsValue> {
        let search = window().ok_or("window unavailable")?.location().search()?;
        UrlSearchParams::new_with_str(&search)
    }

    fn parse_language(value: &str) -> Language {
        if value.to_ascii_lowercase().starts_with("zh") {
            Language::Zh
        } else {
            Language::En
        }
    }

    fn browser_language() -> Language {
        window()
            .and_then(|value| value.navigator().language())
            .map(|value| parse_language(&value))
            .unwrap_or_default()
    }

    fn storage() -> Option<web_sys::Storage> {
        window().and_then(|value| value.local_storage().ok().flatten())
    }

    fn load_scenario() -> Option<ScenarioId> {
        storage()?
            .get_item("iptools.web.v1.scenario")
            .ok()
            .flatten()
            .and_then(|value| ScenarioId::from_str(&value).ok())
    }

    fn load_language() -> Option<Language> {
        storage()?
            .get_item("iptools.web.v1.language")
            .ok()
            .flatten()
            .map(|value| parse_language(&value))
    }

    fn load_renderer() -> Option<String> {
        storage()?
            .get_item("iptools.web.v1.renderer")
            .ok()
            .flatten()
            .filter(|value| matches!(value.as_str(), "canvas" | "dom"))
    }

    fn load_scan_concurrency() -> Option<usize> {
        storage()?
            .get_item("iptools.web.v1.scan_concurrency")
            .ok()
            .flatten()?
            .parse::<usize>()
            .ok()
            .map(|value| value.clamp(10, 500))
    }

    fn load_config() -> Option<ConfigData> {
        let value = storage()?
            .get_item("iptools.web.v1.config")
            .ok()
            .flatten()?;
        serde_json::from_str(&value).ok()
    }

    fn persist_config(config: &ConfigData) {
        let Some(storage) = storage() else {
            return;
        };
        if let Ok(value) = serde_json::to_string(config) {
            let _ = storage.set_item("iptools.web.v1.config", &value);
        }
    }

    fn persist_preferences(scenario: ScenarioId, config: &ConfigData) {
        let Some(storage) = storage() else {
            return;
        };
        let _ = storage.set_item("iptools.web.v1.scenario", scenario.as_str());
        persist_config(config);
        persist_legacy_preferences(config);
    }

    fn persist_legacy_preferences(config: &ConfigData) {
        let Some(storage) = storage() else {
            return;
        };
        let _ = storage.set_item(
            "iptools.web.v1.language",
            if config.language == Language::Zh {
                "zh"
            } else {
                "en"
            },
        );
        let _ = storage.set_item(
            "iptools.web.v1.scan_concurrency",
            &config.scan_concurrency.to_string(),
        );
    }

    fn persist_renderer(renderer: &str) {
        if let Some(storage) = storage() {
            let _ = storage.set_item("iptools.web.v1.renderer", renderer);
        }
    }

    fn replace_language_query(language: Language) -> Result<(), JsValue> {
        let params = query_params()?;
        params.set("lang", if language == Language::Zh { "zh" } else { "en" });
        let search = params.to_string().as_string().unwrap_or_default();
        let window = window().ok_or("window unavailable")?;
        let path = window.location().pathname()?;
        window.history()?.replace_state_with_url(
            &JsValue::NULL,
            "",
            Some(&format!("{path}?{search}")),
        )
    }

    fn browser_clock() -> String {
        let date = js_sys::Date::new_0();
        format!(
            "{:04}-{:02}-{:02} {:02}:{:02}:{:02}",
            date.get_full_year(),
            date.get_month() + 1,
            date.get_date(),
            date.get_hours(),
            date.get_minutes(),
            date.get_seconds(),
        )
    }

    fn performance_now() -> f64 {
        window()
            .and_then(|value| value.performance())
            .map(|value| value.now())
            .unwrap_or_default()
    }

    fn focus_terminal() {
        if let Some(terminal) = window()
            .and_then(|value| value.document())
            .and_then(|document| document.get_element_by_id("terminal"))
            .and_then(|element| element.dyn_into::<web_sys::HtmlElement>().ok())
        {
            let _ = terminal.focus();
        }
    }

    fn mark_rendered(input_generation: u64, state_revision: u64) {
        if let Some(terminal) = window()
            .and_then(|value| value.document())
            .and_then(|document| document.get_element_by_id("terminal"))
        {
            let _ = terminal.set_attribute(
                "data-pending-input-generation",
                &input_generation.to_string(),
            );
            let _ =
                terminal.set_attribute("data-pending-state-revision", &state_revision.to_string());
            let second_frame = Closure::once_into_js(move || {
                if let Some(terminal) = window()
                    .and_then(|value| value.document())
                    .and_then(|document| document.get_element_by_id("terminal"))
                {
                    let _ = terminal.set_attribute(
                        "data-rendered-input-generation",
                        &input_generation.to_string(),
                    );
                    let _ = terminal
                        .set_attribute("data-rendered-state-revision", &state_revision.to_string());
                }
            });
            let first_frame = Closure::once_into_js(move || {
                if let Some(window) = window() {
                    let _ = window.request_animation_frame(second_frame.unchecked_ref());
                }
            });
            if let Some(window) = window() {
                let _ = window.request_animation_frame(first_frame.unchecked_ref());
            }
        }
    }

    fn js_error(error: impl std::fmt::Display) -> JsValue {
        JsValue::from_str(&error.to_string())
    }
}
