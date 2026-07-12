//! Backend-independent Ratatui rendering for iptools.

use iptools_core::{
    Action, AdapterApplyOutcome, AdapterEditPhase, AdapterField, AdapterValidationError, AppModel,
    DiagnosticFocus, DiagnosticTool, Language, LinkQualityDimensionKind, LinkQualityGrade, Page,
    RuntimeErrorCode, TaskStatus,
};
use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{
        Block, Borders, Cell, Clear, Gauge, List, ListItem, Paragraph, Row, Sparkline, Table, Wrap,
    },
};
use unicode_width::UnicodeWidthStr;

// Keep the v0.3.1 terminal palette as the visual contract for every backend.
// Web-specific presentation belongs to the surrounding page, not the TUI.
const PRIMARY: Color = Color::Green;
const SECONDARY: Color = Color::Cyan;
const MUTED: Color = Color::Gray;
const SUBTLE: Color = Color::DarkGray;
const SELECTED: Color = Color::DarkGray;

/// Ephemeral layout state. No application or platform state is stored here.
#[derive(Debug, Default)]
pub struct UiState {
    page_regions: Vec<(Rect, Page)>,
    diagnostic_regions: Vec<(Rect, DiagnosticTool)>,
    scanner_action: Option<Rect>,
    diagnostic_main: Option<Rect>,
    diagnostic_fields: Vec<(Rect, usize, u16)>,
    adapter_regions: Vec<(Rect, usize)>,
    adapter_fields: Vec<(Rect, AdapterField, u16)>,
    settings_regions: Vec<(Rect, usize)>,
}

impl UiState {
    pub fn hit_test(&self, column: u16, row: u16) -> Option<Action> {
        if let Some((area, field, value_x)) = self
            .adapter_fields
            .iter()
            .find(|(area, _, _)| contains(*area, column, row))
        {
            let cursor = column.saturating_sub(*value_x).min(area.width) as usize;
            return Some(Action::SelectAdapterField(*field, cursor));
        }
        if let Some((_, index)) = self
            .adapter_regions
            .iter()
            .find(|(area, _)| contains(*area, column, row))
        {
            return Some(Action::SelectAdapter(*index));
        }
        if let Some((_, index)) = self
            .settings_regions
            .iter()
            .find(|(area, _)| contains(*area, column, row))
        {
            return Some(Action::SelectSetting(*index));
        }
        if let Some((_, page)) = self
            .page_regions
            .iter()
            .find(|(area, _)| contains(*area, column, row))
        {
            return Some(Action::SelectPage(*page as u8));
        }
        if let Some((_, tool)) = self
            .diagnostic_regions
            .iter()
            .find(|(area, _)| contains(*area, column, row))
        {
            return Some(Action::SelectDiagnostic(*tool as u8));
        }
        if let Some((area, index, value_x)) = self
            .diagnostic_fields
            .iter()
            .find(|(area, _, _)| contains(*area, column, row))
        {
            return Some(Action::SelectDiagnosticField(
                *index,
                column.saturating_sub(*value_x).min(area.width) as usize,
            ));
        }
        if self
            .diagnostic_main
            .is_some_and(|area| contains(area, column, row))
        {
            return Some(Action::FocusDiagnostic(DiagnosticFocus::Main));
        }
        if self
            .scanner_action
            .is_some_and(|area| contains(area, column, row))
        {
            return Some(Action::Toggle);
        }
        None
    }
}

/// Render the shared application model using any Ratatui backend.
pub fn render(frame: &mut Frame, model: &AppModel, ui: &mut UiState) {
    *ui = UiState::default();
    let areas = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(0),
            Constraint::Length(1),
        ])
        .split(frame.area());

    render_tabs(frame, areas[0], model, ui);
    match model.page {
        Page::Dashboard => render_dashboard(frame, areas[1], model),
        Page::Adapters => render_adapters(frame, areas[1], model, ui),
        Page::Scanner => render_scanner(frame, areas[1], model, ui),
        Page::Traffic => render_traffic(frame, areas[1], model),
        Page::Diagnostics => render_diagnostics(frame, areas[1], model, ui),
        Page::Settings => render_settings(frame, areas[1], model, ui),
    }
    render_footer(frame, areas[2], model);

    if model.show_help {
        render_help(frame, model.language);
    }
}

fn render_tabs(frame: &mut Frame, area: Rect, model: &AppModel, ui: &mut UiState) {
    let block = Block::default().borders(Borders::ALL).title(if model.demo {
        " IP Tools CLI · DEMO "
    } else {
        " IP Tools CLI "
    });
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let mut x = inner.x;
    for (index, page) in Page::ALL.into_iter().enumerate() {
        if x >= inner.right() {
            break;
        }
        let label = format!(" {} ", page_label(page, model.language));
        let width = label.width() as u16;
        let tab = Rect::new(x, inner.y, width.min(inner.right().saturating_sub(x)), 1);
        let style = if model.page == page {
            Style::default()
                .fg(
                    if model.page == Page::Diagnostics && model.diagnostics.focused {
                        Color::White
                    } else {
                        PRIMARY
                    },
                )
                .bg(SELECTED)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(PRIMARY)
        };
        frame.render_widget(Paragraph::new(label).style(style), tab);
        ui.page_regions.push((tab, page));
        x = x.saturating_add(width);
        if index + 1 < Page::ALL.len() && x < inner.right() {
            frame.render_widget(
                Paragraph::new("|").style(Style::default().fg(MUTED)),
                Rect::new(x, inner.y, 1, 1),
            );
            x = x.saturating_add(1);
        }
    }
}

fn render_dashboard(frame: &mut Frame, area: Rect, model: &AppModel) {
    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(area);
    let snapshot = &model.dashboard.snapshot;
    let key = Style::default().fg(MUTED);
    let mut local = vec![
        Row::new(vec![
            Cell::from(Span::styled(tr(model.language, "时间", "Time"), key)),
            Cell::from(snapshot.observed_at.clone()),
        ]),
        Row::new(vec![
            Cell::from(Span::styled(tr(model.language, "主机名", "Hostname"), key)),
            Cell::from(format!(
                "{} ({} {})",
                snapshot.hostname, snapshot.os_name, snapshot.os_version
            )),
        ]),
        Row::new(vec![Cell::from(""), Cell::from("")]),
    ];
    if let Some(interface) = &snapshot.active_interface {
        let name = interface.ssid.as_ref().map_or_else(
            || interface.name.clone(),
            |ssid| format!("{} (SSID: {ssid})", interface.name),
        );
        local.push(
            Row::new(vec![
                Cell::from(Span::styled(
                    tr(model.language, "活动接口", "Interface"),
                    key,
                )),
                Cell::from(vec![
                    Line::from(Span::styled(
                        name,
                        Style::default().fg(SECONDARY).add_modifier(Modifier::BOLD),
                    )),
                    Line::from(Span::styled(
                        interface.description.clone(),
                        Style::default().fg(SUBTLE),
                    )),
                ]),
            ])
            .height(2),
        );
        local.push(Row::new(vec![
            Cell::from(Span::styled(
                tr(model.language, "连接 / 寻址", "Connection"),
                key,
            )),
            Cell::from(format!(
                "{} / {}",
                if interface.is_physical {
                    tr(model.language, "物理", "Physical")
                } else {
                    tr(model.language, "虚拟", "Virtual")
                },
                if interface.dhcp_enabled {
                    "DHCP"
                } else {
                    tr(model.language, "静态", "Static")
                }
            )),
        ]));
        local.push(Row::new(vec![
            Cell::from(Span::styled(tr(model.language, "本地 IP", "Local IP"), key)),
            Cell::from(interface.ipv4.clone()),
        ]));
        local.push(Row::new(vec![Cell::from(""), Cell::from("")]));
    } else {
        local.push(Row::new(vec![
            Cell::from(Span::styled(tr(model.language, "状态", "Status"), key)),
            Cell::from(Span::styled(
                tr(model.language, "无活动接口", "No active interface"),
                Style::default().fg(Color::Red),
            )),
        ]));
    }
    local.extend([
        Row::new(vec![
            Cell::from(Span::styled(
                tr(model.language, "实时速率", "Live Rate"),
                key,
            )),
            Cell::from(Line::from(vec![
                Span::styled(
                    format!("↓ {:<10}", format_rate(snapshot.download_bps)),
                    Style::default().fg(Color::Green),
                ),
                Span::styled(
                    format!("↑ {:<10}", format_rate(snapshot.upload_bps)),
                    Style::default().fg(Color::Yellow),
                ),
            ])),
        ]),
        Row::new(vec![
            Cell::from(Span::styled(
                tr(model.language, "累计流量", "Traffic Total"),
                key,
            )),
            Cell::from(format!(
                "{}: {:<10}{}: {:<10}",
                tr(model.language, "接收", "RX"),
                format_bytes(snapshot.total_download),
                tr(model.language, "发送", "TX"),
                format_bytes(snapshot.total_upload)
            )),
        ]),
    ]);
    frame.render_widget(
        Table::new(local, [Constraint::Length(14), Constraint::Min(0)])
            .column_spacing(1)
            .block(Block::bordered().title(tr(model.language, " 本地概览 ", " Local Overview "))),
        cols[0],
    );
    let proxy = snapshot
        .proxy
        .as_deref()
        .unwrap_or(tr(model.language, "无", "none"));
    let mut public = vec![
        Row::new(vec![
            Cell::from(Span::styled(tr(model.language, "系统代理", "Proxy"), key)),
            Cell::from(Span::styled(
                proxy,
                Style::default().fg(if snapshot.proxy.is_some() {
                    Color::Yellow
                } else {
                    SUBTLE
                }),
            )),
        ]),
        Row::new(vec![Cell::from(""), Cell::from("")]),
    ];
    if let Some(info) = &snapshot.public_info {
        let location = [&info.city, &info.region, &info.country]
            .into_iter()
            .filter(|part| !part.is_empty())
            .cloned()
            .collect::<Vec<_>>()
            .join(", ");
        public.extend([
            Row::new(vec![
                Cell::from(Span::styled("Public IP", key)),
                Cell::from(Span::styled(
                    info.ip.clone(),
                    Style::default()
                        .fg(Color::LightCyan)
                        .add_modifier(Modifier::BOLD),
                )),
            ]),
            Row::new(vec![
                Cell::from(Span::styled(
                    tr(model.language, "地理位置", "Location"),
                    key,
                )),
                Cell::from(location),
            ]),
            Row::new(vec![
                Cell::from(Span::styled("ISP", key)),
                Cell::from(info.isp.clone()),
            ]),
        ]);
    } else {
        public.push(Row::new(vec![
            Cell::from(Span::styled("Public IP", key)),
            Cell::from(Span::styled(
                task_label(&model.dashboard.status, model.language),
                Style::default().fg(Color::Yellow),
            )),
        ]));
    }
    if let Some(error) = &model.dashboard.error {
        public.push(Row::new(vec![
            Cell::from(Span::styled(tr(model.language, "错误", "Error"), key)),
            Cell::from(Span::styled(
                error.message.clone(),
                Style::default().fg(Color::Red),
            )),
        ]));
    }
    if model.demo {
        public.extend([
            Row::new(vec![Cell::from(""), Cell::from("")]),
            Row::new(vec![
                Cell::from(Span::styled(tr(model.language, "说明", "Note"), key)),
                Cell::from(Span::styled(
                    tr(
                        model.language,
                        "演示模式只使用模拟数据，不访问您的局域网。",
                        "Demo mode uses simulated data and never accesses your LAN.",
                    ),
                    Style::default().fg(SUBTLE),
                )),
            ]),
        ]);
    }
    frame.render_widget(
        Table::new(public, [Constraint::Length(14), Constraint::Min(0)])
            .column_spacing(1)
            .block(Block::bordered().title(tr(model.language, " 公网信息 ", " Public Network "))),
        cols[1],
    );
}

fn render_adapters(frame: &mut Frame, area: Rect, model: &AppModel, ui: &mut UiState) {
    if let Some(edit) = &model.adapters.edit {
        render_adapter_edit(frame, area, model, edit, ui);
        return;
    }
    let cols =
        Layout::horizontal([Constraint::Percentage(30), Constraint::Percentage(70)]).split(area);
    let list_inner = Block::bordered().inner(cols[0]);
    for index in 0..model.adapters.items.len().min(list_inner.height as usize) {
        ui.adapter_regions.push((
            Rect::new(
                list_inner.x,
                list_inner.y + index as u16,
                list_inner.width,
                1,
            ),
            index,
        ));
    }
    let items = model
        .adapters
        .items
        .iter()
        .enumerate()
        .map(|(index, adapter)| {
            let selected = index == model.adapters.selected;
            let prefix = if adapter.is_physical { "[P] " } else { "[V] " };
            let up = adapter_is_up(adapter);
            ListItem::new(Line::from(vec![
                Span::styled(
                    if selected { "> " } else { "  " },
                    Style::default().fg(PRIMARY),
                ),
                Span::styled(prefix, Style::default().fg(SECONDARY)),
                Span::styled(
                    adapter.name.clone(),
                    Style::default().fg(if up { Color::White } else { SUBTLE }),
                ),
            ]))
            .style(if selected {
                Style::default().bg(SELECTED)
            } else {
                Style::default()
            })
        });
    frame.render_widget(
        List::new(items).block(Block::bordered().title(tr(
            model.language,
            " 网络适配器 ",
            " Network Adapters ",
        ))),
        cols[0],
    );

    let detail_block = Block::bordered()
        .title(tr(model.language, " 适配器详情 ", " Adapter Details "))
        .title(
            Line::from(Span::styled(
                tr(model.language, " [E] 编辑 ", " [E] Edit "),
                Style::default().fg(SECONDARY),
            ))
            .alignment(Alignment::Right),
        );
    if let Some(adapter) = model.adapters.items.get(model.adapters.selected) {
        let key = Style::default().fg(MUTED);
        let value = Style::default().fg(Color::White);
        let mut rows = vec![
            Row::new(vec![
                Cell::from(Span::styled(
                    tr(model.language, "名称 / 描述", "Name / Description"),
                    key,
                )),
                Cell::from(vec![
                    Line::from(Span::styled(
                        adapter.name.clone(),
                        Style::default().fg(SECONDARY).add_modifier(Modifier::BOLD),
                    )),
                    Line::from(Span::styled(
                        adapter.description.clone(),
                        Style::default().fg(SUBTLE),
                    )),
                ]),
            ])
            .height(2),
            Row::new(vec![
                Cell::from(Span::styled(tr(model.language, "状态", "Status"), key)),
                Cell::from(Line::from(vec![
                    Span::styled(
                        adapter.status.clone(),
                        Style::default().fg(if adapter_is_up(adapter) {
                            Color::Green
                        } else {
                            Color::Red
                        }),
                    ),
                    Span::raw("  (SSID: "),
                    Span::styled(
                        adapter.ssid.clone().unwrap_or_else(|| "-".into()),
                        Style::default().fg(if adapter.ssid.is_some() {
                            Color::Yellow
                        } else {
                            SUBTLE
                        }),
                    ),
                    Span::raw(")"),
                ])),
            ]),
            Row::new(vec![
                Cell::from(Span::styled(
                    tr(model.language, "连接类型", "Connection Type"),
                    key,
                )),
                Cell::from(format!(
                    "{} [{}]",
                    if adapter.is_physical {
                        tr(model.language, "物理", "Physical")
                    } else {
                        tr(model.language, "虚拟", "Virtual")
                    },
                    adapter.kind
                )),
            ]),
            Row::new(vec![
                Cell::from(Span::styled(tr(model.language, "IP 类型", "IP Type"), key)),
                Cell::from(if adapter.dhcp_enabled {
                    "DHCP"
                } else {
                    tr(model.language, "静态", "Static")
                }),
            ]),
            Row::new(vec![
                Cell::from(Span::styled("MAC", key)),
                Cell::from(adapter.mac.clone()),
            ]),
            Row::new(vec![
                Cell::from(Span::styled("IPv4", key)),
                Cell::from(format!(
                    "• {}",
                    adapter.cidr.as_deref().unwrap_or(&adapter.ipv4)
                )),
            ]),
        ];
        rows.push(
            Row::new(vec![
                Cell::from(Span::styled("IPv6", key)),
                Cell::from(if adapter.ipv6.is_empty() {
                    vec![Line::from("-")]
                } else {
                    adapter
                        .ipv6
                        .iter()
                        .map(|ip| Line::from(format!("• {ip}")))
                        .collect()
                }),
            ])
            .height(adapter.ipv6.len().max(1) as u16),
        );
        rows.push(Row::new(vec![Cell::from(""), Cell::from("")]));
        rows.push(Row::new(vec![
            Cell::from(Span::styled(
                tr(model.language, "实时流量", "Traffic Rate"),
                key,
            )),
            Cell::from(Line::from(vec![
                Span::styled(
                    format!("↓ {:<10}", format_rate(adapter.download_bps)),
                    Style::default().fg(Color::Green),
                ),
                Span::styled(
                    format!("↑ {:<10}", format_rate(adapter.upload_bps)),
                    Style::default().fg(Color::Yellow),
                ),
            ])),
        ]));
        rows.push(Row::new(vec![
            Cell::from(Span::styled(
                tr(model.language, "累计流量", "Traffic Total"),
                key,
            )),
            Cell::from(format!(
                "{}: {:<10}{}: {:<10}",
                tr(model.language, "接收", "RX"),
                format_bytes(adapter.total_download),
                tr(model.language, "发送", "TX"),
                format_bytes(adapter.total_upload)
            )),
        ]));
        frame.render_widget(
            Table::new(rows, [Constraint::Length(16), Constraint::Min(0)])
                .column_spacing(1)
                .style(value)
                .block(detail_block),
            cols[1],
        );
    } else {
        frame.render_widget(
            Paragraph::new(tr(
                model.language,
                "未发现网络适配器。",
                "No network adapters detected.",
            ))
            .block(detail_block),
            cols[1],
        );
    }
}

fn adapter_is_up(adapter: &iptools_core::AdapterInfo) -> bool {
    let status = adapter.status.to_ascii_lowercase();
    !adapter.ipv4.is_empty()
        && adapter.ipv4 != "—"
        && !["down", "disconnected", "standby", "internal"]
            .iter()
            .any(|marker| status.contains(marker))
}

fn render_adapter_edit(
    frame: &mut Frame,
    area: Rect,
    model: &AppModel,
    edit: &iptools_core::AdapterEditState,
    ui: &mut UiState,
) {
    let block = Block::bordered().title(format!(
        " {} — {} ",
        tr(model.language, "编辑适配器", "Edit Adapter"),
        edit.name
    ));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let rows = Layout::vertical([
        Constraint::Length(6),
        Constraint::Min(3),
        Constraint::Length(2),
    ])
    .split(inner);

    let labels = [
        tr(model.language, "模式", "Mode"),
        "IPv4",
        tr(model.language, "子网掩码", "Subnet mask"),
        tr(model.language, "网关", "Gateway"),
        tr(model.language, "首选 DNS", "Primary DNS"),
        tr(model.language, "备用 DNS", "Secondary DNS"),
    ];
    for (index, field) in AdapterField::ALL.into_iter().enumerate() {
        let row = Rect::new(rows[0].x, rows[0].y + index as u16, rows[0].width, 1);
        let label_width = 18.min(row.width);
        let value_x = row.x.saturating_add(label_width);
        if edit.phase == AdapterEditPhase::Editing {
            ui.adapter_fields.push((row, field, value_x));
        }
        let selected = field == edit.selected;
        let enabled = field == AdapterField::Mode || !edit.params.use_dhcp;
        let style = if !enabled {
            Style::default().fg(MUTED)
        } else if selected {
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::White)
        };
        let value = match field {
            AdapterField::Mode => tr(
                model.language,
                if edit.params.use_dhcp {
                    "自动 (DHCP)"
                } else {
                    "手动 (静态)"
                },
                if edit.params.use_dhcp {
                    "Automatic (DHCP)"
                } else {
                    "Manual (static)"
                },
            )
            .to_string(),
            _ => edit.value(field).to_string(),
        };
        let label_area = Rect::new(row.x, row.y, label_width, 1);
        let value_area = Rect::new(value_x, row.y, row.right().saturating_sub(value_x), 1);
        frame.render_widget(
            Paragraph::new(format!(
                "{} {}",
                if selected { ">" } else { " " },
                labels[index]
            ))
            .style(if selected {
                Style::default().fg(Color::Yellow)
            } else {
                Style::default().fg(MUTED)
            }),
            label_area,
        );
        let value_widget = if selected
            && enabled
            && field != AdapterField::Mode
            && edit.phase == AdapterEditPhase::Editing
        {
            let raw = edit.value(field);
            let suffix = edit.history.iter().find_map(|candidate| {
                (!raw.is_empty()
                    && edit.cursor == raw.len()
                    && candidate.starts_with(raw)
                    && candidate.len() > raw.len())
                .then(|| candidate[raw.len()..].to_string())
            });
            let before = &raw[..edit.cursor.min(raw.len())];
            let after = &raw[edit.cursor.min(raw.len())..];
            let mut spans = vec![
                Span::styled(before.to_string(), style),
                Span::styled("█", style),
                Span::styled(after.to_string(), style),
            ];
            if let Some(suffix) = suffix {
                spans.push(Span::styled(suffix, Style::default().fg(MUTED)));
            }
            Paragraph::new(Line::from(spans))
        } else {
            Paragraph::new(if value.is_empty() {
                "—".into()
            } else {
                value
            })
            .style(style)
        };
        frame.render_widget(value_widget, value_area);
    }

    let validation = edit.validation_error.map(|error| match error {
        AdapterValidationError::Ipv4 => {
            tr(model.language, "IPv4 地址无效。", "Invalid IPv4 address.")
        }
        AdapterValidationError::Mask => tr(
            model.language,
            "子网掩码必须连续，且不能是 /0 或 /32。",
            "Subnet mask must be contiguous and cannot be /0 or /32.",
        ),
        AdapterValidationError::Gateway => {
            tr(model.language, "网关地址无效。", "Invalid gateway address.")
        }
        AdapterValidationError::Dns => tr(model.language, "DNS 地址无效。", "Invalid DNS address."),
    });
    let status = match &edit.phase {
        AdapterEditPhase::Editing => validation.unwrap_or(tr(
            model.language,
            "静态模式下可编辑地址；Ctrl+R 打开历史。",
            "Address fields are editable in static mode; Ctrl+R opens history.",
        )),
        AdapterEditPhase::Confirming => tr(
            model.language,
            "确认应用此网络配置？Enter 应用，Esc 返回。",
            "Apply this network configuration? Enter applies; Esc returns.",
        ),
        AdapterEditPhase::Applying => tr(
            model.language,
            "正在应用，请稍候…",
            "Applying; please wait…",
        ),
        AdapterEditPhase::Succeeded(AdapterApplyOutcome::Persistent) => tr(
            model.language,
            "配置已永久应用。按任意键返回。",
            "Configuration applied persistently. Press any key to return.",
        ),
        AdapterEditPhase::Succeeded(AdapterApplyOutcome::RuntimeOnly) => tr(
            model.language,
            "配置仅在本次运行中生效，重启后可能恢复。按任意键返回。",
            "Configuration is runtime-only and may reset after reboot. Press any key to return.",
        ),
        AdapterEditPhase::Succeeded(AdapterApplyOutcome::Simulated) => tr(
            model.language,
            "模拟配置已应用；未修改真实系统。按任意键返回。",
            "Simulated configuration applied; no real system was changed. Press any key to return.",
        ),
        AdapterEditPhase::Failed(error) => &error.message,
    };
    let status_style = match edit.phase {
        AdapterEditPhase::Failed(_) => Style::default().fg(Color::Red),
        AdapterEditPhase::Succeeded(AdapterApplyOutcome::RuntimeOnly) => {
            Style::default().fg(Color::Yellow)
        }
        AdapterEditPhase::Succeeded(_) => Style::default().fg(Color::Green),
        _ => Style::default().fg(PRIMARY),
    };
    frame.render_widget(
        Paragraph::new(status)
            .style(status_style)
            .wrap(Wrap { trim: true }),
        rows[1],
    );
    frame.render_widget(
        Paragraph::new(tr(
            model.language,
            " [↑↓] 字段  [←→] 光标/模式  [Enter] 确认  [Esc] 取消 ",
            " [↑↓] Fields  [←→] Cursor/mode  [Enter] Confirm  [Esc] Cancel ",
        ))
        .style(Style::default().fg(MUTED)),
        rows[2],
    );

    if edit.history_open {
        let popup = centered(area, 52, 45);
        frame.render_widget(Clear, popup);
        let items = edit
            .history
            .iter()
            .take(8)
            .enumerate()
            .map(|(index, value)| {
                ListItem::new(value.clone()).style(if index == edit.history_selected {
                    Style::default().bg(SELECTED).fg(Color::White)
                } else {
                    Style::default()
                })
            });
        frame.render_widget(
            List::new(items).block(Block::bordered().title(tr(
                model.language,
                " 输入历史 ",
                " Input history ",
            ))),
            popup,
        );
    }

    if edit.phase == AdapterEditPhase::Confirming {
        let popup = centered(area, 64, 30);
        frame.render_widget(Clear, popup);
        let warning = if model.demo {
            tr(
                model.language,
                "只会修改当前模拟场景，不会触及真实系统。\n\nEnter / Space  确认模拟应用\nEsc            返回检查",
                "Only the current simulation will change; the real system is untouched.\n\nEnter / Space  apply simulation\nEsc            review",
            )
        } else {
            tr(
                model.language,
                "即将修改系统网络配置。\n\nEnter / Space  确认应用\nEsc            返回检查",
                "The system network configuration is about to change.\n\nEnter / Space  apply\nEsc            review",
            )
        };
        frame.render_widget(
            Paragraph::new(warning)
                .block(Block::bordered().title(tr(model.language, " 确认应用 ", " Confirm apply ")))
                .alignment(Alignment::Center)
                .wrap(Wrap { trim: true }),
            popup,
        );
    }
}

fn render_scanner(frame: &mut Frame, area: Rect, model: &AppModel, ui: &mut UiState) {
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(0),
            Constraint::Length(1),
        ])
        .split(area);
    let input_style = if model.scanner.editing {
        Style::default().fg(Color::Yellow)
    } else {
        Style::default().fg(Color::White)
    };
    let action = if model.scanner.status == TaskStatus::Running {
        tr(model.language, "停止", "Stop")
    } else {
        tr(model.language, "开始", "Start")
    };
    let status = task_label(&model.scanner.status, model.language).trim();
    let count = if model.scanner.total == 0 {
        "—".into()
    } else {
        model.scanner.total.to_string()
    };
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(
                format!(" {} ", tr(model.language, "扫描范围", "Range")),
                input_style,
            ),
            Span::styled(model.scanner.cidr.clone(), input_style),
            Span::styled(if model.scanner.editing { "█" } else { "" }, input_style),
            Span::styled(
                format!(
                    "   {} {}   [{}]   {} / {}",
                    tr(model.language, "地址数", "Addresses"),
                    count,
                    status,
                    tr(model.language, "E 编辑", "E Edit"),
                    action,
                ),
                input_style,
            ),
        ]))
        .block(Block::bordered().title(tr(
            model.language,
            " 局域网扫描 ",
            " LAN Scanner ",
        ))),
        rows[0],
    );
    ui.scanner_action = Some(rows[0]);
    let ratio = if model.scanner.total == 0 {
        0.0
    } else {
        model.scanner.current as f64 / model.scanner.total as f64
    };
    let table_rows = model
        .scanner
        .results
        .iter()
        .enumerate()
        .map(|(index, host)| {
            Row::new(vec![
                format!(
                    "{}{}",
                    if index == model.scanner.selected {
                        ">> "
                    } else {
                        "   "
                    },
                    host.ip
                ),
                host.mac.clone(),
                if host.vendor.is_empty() {
                    "-".into()
                } else {
                    host.vendor.clone()
                },
                host.hostname.clone(),
            ])
            .style(if index == model.scanner.selected {
                Style::default()
                    .fg(SECONDARY)
                    .bg(SELECTED)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            })
        });
    frame.render_widget(
        Table::new(
            table_rows,
            [
                Constraint::Percentage(22),
                Constraint::Percentage(28),
                Constraint::Percentage(22),
                Constraint::Percentage(28),
            ],
        )
        .header(
            Row::new([
                format!("   {}", tr(model.language, "IP 地址", "IP Address")),
                "MAC".into(),
                tr(model.language, "厂商", "Vendor").into(),
                tr(model.language, "主机名", "Hostname").into(),
            ])
            .style(Style::default().fg(MUTED))
            .bottom_margin(1),
        )
        .block(Block::bordered().title(format!(
            " {} ({}) ",
            tr(model.language, "发现设备", "Devices Found"),
            model.scanner.results.len()
        ))),
        rows[1],
    );

    if matches!(model.scanner.status, TaskStatus::Running | TaskStatus::Done) {
        frame.render_widget(
            Gauge::default()
                .gauge_style(
                    Style::default()
                        .fg(if model.scanner.status == TaskStatus::Done && ratio < 1.0 {
                            Color::Yellow
                        } else {
                            SECONDARY
                        })
                        .bg(SELECTED),
                )
                .ratio(ratio.clamp(0.0, 1.0))
                .label(format!(
                    "{:.1}% ({}/{})",
                    ratio * 100.0,
                    model.scanner.current,
                    model.scanner.total
                )),
            rows[2],
        );
    }
}

fn render_traffic(frame: &mut Frame, area: Rect, model: &AppModel) {
    let rows = model.traffic.rows.iter().enumerate().map(|(index, row)| {
        Row::new(vec![
            row.name.clone(),
            format_rate(row.download_bps),
            format_rate(row.upload_bps),
            format!(
                "{} / {}",
                format_bytes(row.session_download),
                format_bytes(row.session_upload)
            ),
            format!(
                "{} / {}",
                format_bytes(row.total_download),
                format_bytes(row.total_upload)
            ),
        ])
        .style(if index == model.traffic.selected {
            Style::default().bg(SELECTED)
        } else {
            Style::default()
        })
    });
    frame.render_widget(
        Table::new(
            rows,
            [
                Constraint::Percentage(28),
                Constraint::Length(14),
                Constraint::Length(14),
                Constraint::Length(14),
                Constraint::Min(12),
            ],
        )
        .header(
            Row::new(vec![
                Cell::from(tr(model.language, "接口名称", "Interface Name"))
                    .style(Style::default().fg(MUTED)),
                Cell::from(tr(model.language, "接收速率", "RX Rate"))
                    .style(Style::default().fg(Color::Green)),
                Cell::from(tr(model.language, "发送速率", "TX Rate"))
                    .style(Style::default().fg(Color::Yellow)),
                Cell::from(tr(model.language, "本次接收/发送", "Session RX/TX"))
                    .style(Style::default().fg(MUTED)),
                Cell::from(tr(model.language, "累计接收/发送", "Total RX/TX"))
                    .style(Style::default().fg(MUTED)),
            ])
            .bottom_margin(1),
        )
        .block(Block::bordered().title(tr(model.language, " 实时流量 ", " Live Traffic "))),
        area,
    );
}

fn render_diagnostics(frame: &mut Frame, area: Rect, model: &AppModel, ui: &mut UiState) {
    let common = model.diagnostics.active_common();
    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(20),
            Constraint::Percentage(50),
            Constraint::Percentage(30),
        ])
        .split(area);

    let focus_style = |focus| {
        if model.diagnostics.focused && model.diagnostics.focus == focus {
            Style::default().fg(Color::Yellow)
        } else {
            Style::default().fg(MUTED)
        }
    };

    let mut items = Vec::new();
    let menu_inner = Block::bordered().inner(cols[0]);
    for (index, tool) in DiagnosticTool::ALL.into_iter().enumerate() {
        let row = Rect::new(
            menu_inner.x,
            menu_inner.y + index as u16,
            menu_inner.width,
            1,
        );
        ui.diagnostic_regions.push((row, tool));
        let selected = tool == model.diagnostics.tool;
        items.push(
            ListItem::new(format!(
                "{}{}",
                if selected { "> " } else { "  " },
                tool_label(tool, model.language)
            ))
            .style(if selected {
                Style::default()
                    .fg(Color::White)
                    .bg(SELECTED)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::White)
            }),
        );
    }
    frame.render_widget(
        List::new(items).block(
            Block::bordered()
                .title(tr(model.language, " 诊断工具 ", " Diagnostic Tools "))
                .border_style(focus_style(DiagnosticFocus::Menu)),
        ),
        cols[0],
    );

    ui.diagnostic_main = Some(cols[1]);
    let main_block = Block::bordered()
        .title(tr(model.language, " 主面板 ", " Main Panel "))
        .border_style(focus_style(DiagnosticFocus::Main));
    let main_inner = main_block.inner(cols[1]);
    frame.render_widget(main_block, cols[1]);
    if let Some(failure) = diagnostic_failure(common, model.language) {
        let mut lines = vec![
            Line::from(Span::styled(
                tr(model.language, "诊断失败", "Diagnostic failed"),
                Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
            )),
            Line::from(failure),
            Line::from(""),
        ];
        lines.extend(
            common
                .log
                .iter()
                .rev()
                .take(main_inner.height.saturating_sub(4) as usize)
                .rev()
                .cloned()
                .map(Line::from),
        );
        frame.render_widget(Paragraph::new(lines).wrap(Wrap { trim: true }), main_inner);
    } else {
        match model.diagnostics.tool {
            DiagnosticTool::Ping => render_ping(main_inner, frame, model),
            DiagnosticTool::Trace => render_trace(main_inner, frame, model),
            DiagnosticTool::PortScan => render_port_scan(main_inner, frame, model),
            DiagnosticTool::PublicSpeed => render_public_speed(main_inner, frame, model),
            DiagnosticTool::LinkQuality => render_link_quality(main_inner, frame, model),
            DiagnosticTool::LanSpeed => {
                let mut lines = vec![
                    Line::from(Span::styled(
                        common.primary.clone(),
                        Style::default().fg(SECONDARY).add_modifier(Modifier::BOLD),
                    )),
                    Line::from(common.detail.clone()),
                    Line::from(""),
                ];
                lines.extend(
                    common
                        .log
                        .iter()
                        .rev()
                        .take(main_inner.height.saturating_sub(4) as usize)
                        .rev()
                        .cloned()
                        .map(Line::from),
                );
                frame.render_widget(Paragraph::new(lines).wrap(Wrap { trim: true }), main_inner);
            }
        }
    }

    let config_block = Block::bordered()
        .title(tr(model.language, " 配置 ", " Configuration "))
        .border_style(focus_style(DiagnosticFocus::Config));
    let config_inner = config_block.inner(cols[2]);
    frame.render_widget(config_block, cols[2]);
    let fields = diagnostic_fields(model);
    for (index, (label, raw_value)) in fields.into_iter().enumerate() {
        let y = index as u16 * 2;
        if y + 1 >= config_inner.height {
            break;
        }
        let label_row = Rect::new(config_inner.x, config_inner.y + y, config_inner.width, 1);
        let value_row = Rect::new(
            config_inner.x,
            config_inner.y + y + 1,
            config_inner.width,
            1,
        );
        let value_x = value_row.x.saturating_add(3.min(value_row.width));
        ui.diagnostic_fields.push((
            Rect::new(label_row.x, label_row.y, label_row.width, 2),
            index,
            value_x,
        ));
        let selected = model.diagnostics.focused
            && model.diagnostics.focus == DiagnosticFocus::Config
            && active_diagnostic_config_index(model) == index;
        let text_editable = match model.diagnostics.tool {
            DiagnosticTool::Ping => index == 0,
            DiagnosticTool::Trace => true,
            DiagnosticTool::LinkQuality => index >= 1,
            _ => false,
        };
        frame.render_widget(
            Paragraph::new(format!("{label}:")).style(Style::default().fg(if selected {
                Color::Yellow
            } else {
                MUTED
            })),
            label_row,
        );
        let mut spans = vec![Span::styled(
            if selected { ">> " } else { "   " },
            if selected {
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::White)
            },
        )];
        if selected && common.job.is_none() && text_editable {
            let cursor = model.diagnostics.cursor.min(raw_value.len());
            spans.push(Span::styled(
                raw_value[..cursor].to_string(),
                Style::default().fg(Color::Yellow),
            ));
            spans.push(Span::styled("█", Style::default().fg(Color::Yellow)));
            spans.push(Span::styled(
                raw_value[cursor..].to_string(),
                Style::default().fg(Color::Yellow),
            ));
            if index == diagnostic_target_index(model.diagnostics.tool)
                && !raw_value.is_empty()
                && cursor == raw_value.len()
                && let Some(candidate) = model.diagnostics.target_history.iter().find(|candidate| {
                    candidate.starts_with(&raw_value) && candidate.len() > raw_value.len()
                })
            {
                spans.push(Span::styled(
                    candidate[raw_value.len()..].to_string(),
                    Style::default().fg(MUTED),
                ));
            }
        } else {
            spans.push(Span::styled(
                raw_value,
                if common.job.is_some() {
                    Style::default().fg(MUTED)
                } else if selected {
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(Color::White)
                },
            ));
        }
        frame.render_widget(Paragraph::new(Line::from(spans)), value_row);
    }

    if model.diagnostics.history_open {
        let popup = centered(cols[2], 90, 55);
        frame.render_widget(Clear, popup);
        let items = model
            .diagnostics
            .target_history
            .iter()
            .take(8)
            .enumerate()
            .map(|(index, value)| {
                ListItem::new(value.clone()).style(if index == model.diagnostics.history_selected {
                    Style::default().bg(SELECTED)
                } else {
                    Style::default()
                })
            });
        frame.render_widget(
            List::new(items).block(Block::bordered().title(tr(
                model.language,
                " 目标历史 ",
                " Target history ",
            ))),
            popup,
        );
    }

    if !model.diagnostics.focused {
        let hint = centered(cols[1], 70, 18);
        frame.render_widget(
            Paragraph::new(tr(
                model.language,
                "按 Enter 进入诊断工具",
                "Press Enter to focus diagnostics",
            ))
            .alignment(Alignment::Center)
            .style(Style::default().fg(MUTED)),
            hint,
        );
    }
}

fn render_ping(area: Rect, frame: &mut Frame, model: &AppModel) {
    let state = &model.diagnostics.ping;
    let latest = state.samples.last();
    let summary = state.summary.as_ref();
    let min = summary
        .and_then(|value| value.min_ms)
        .or_else(|| latest.and_then(|value| value.min_ms));
    let avg = summary
        .and_then(|value| value.average_ms)
        .or_else(|| latest.and_then(|value| value.average_ms));
    let max = summary
        .and_then(|value| value.max_ms)
        .or_else(|| latest.and_then(|value| value.max_ms));
    let loss = summary
        .map(|value| value.loss_percent)
        .or_else(|| latest.map(|value| value.loss_percent))
        .unwrap_or_default();
    let jitter = if state.samples.len() > 1 {
        let pairs = state
            .samples
            .windows(2)
            .filter_map(|pair| Some(pair[0].latency_ms?.abs_diff(pair[1].latency_ms?) as f64))
            .collect::<Vec<_>>();
        (!pairs.is_empty()).then(|| pairs.iter().sum::<f64>() / pairs.len() as f64)
    } else {
        None
    };
    let stats_area = Rect::new(area.x, area.y, area.width, area.height.min(4));
    let status_area = bottom_row(area);
    let available = status_area.y.saturating_sub(stats_area.bottom());
    let log_height = available.min(8);
    let log_area = Rect::new(
        area.x,
        status_area.y.saturating_sub(log_height),
        area.width,
        log_height,
    );
    let chart_area = Rect::new(
        area.x,
        stats_area.bottom(),
        area.width,
        log_area.y.saturating_sub(stats_area.bottom()),
    );
    let third = stats_area.width / 3;
    let stats = [
        (
            format!(
                "{}: {} ms",
                tr(model.language, "最近", "Last"),
                format_optional_u64(latest.and_then(|value| value.latency_ms))
            ),
            SECONDARY,
        ),
        (
            format!(
                "{}: {} ms",
                tr(model.language, "最小", "Min"),
                format_optional_u64(min)
            ),
            Color::Green,
        ),
        (
            format!(
                "{}: {} ms",
                tr(model.language, "最大", "Max"),
                format_optional_u64(max)
            ),
            Color::Red,
        ),
        (
            format!(
                "{}: {} ms",
                tr(model.language, "平均", "Average"),
                format_optional_f64(avg)
            ),
            Color::White,
        ),
        (
            format!(
                "{}: {} ms",
                tr(model.language, "抖动", "Jitter"),
                format_optional_f64(jitter)
            ),
            Color::White,
        ),
        (
            format!("{}: {:.1}%", tr(model.language, "丢包", "Loss"), loss),
            Color::White,
        ),
    ];
    for (index, (text, color)) in stats.into_iter().enumerate() {
        frame.render_widget(
            Paragraph::new(text).style(Style::default().fg(color)),
            Rect::new(
                stats_area.x + third * (index % 3) as u16,
                stats_area.y + (index / 3) as u16,
                third,
                1,
            ),
        );
    }
    frame.render_widget(
        Block::default()
            .borders(Borders::BOTTOM)
            .border_style(Style::default().fg(SUBTLE)),
        Rect::new(stats_area.x, stats_area.y + 2, stats_area.width, 1),
    );
    let history = state
        .samples
        .iter()
        .map(|sample| sample.latency_ms.unwrap_or_default())
        .collect::<Vec<_>>();
    frame.render_widget(
        Sparkline::default()
            .block(Block::default().title(tr(model.language, "延迟曲线", "Latency History")))
            .data(&history)
            .style(Style::default().fg(PRIMARY)),
        chart_area,
    );
    let logs = state
        .common
        .log
        .iter()
        .rev()
        .map(|entry| ListItem::new(entry.clone()));
    frame.render_widget(
        List::new(logs).block(
            Block::default()
                .borders(Borders::TOP)
                .title(tr(model.language, "日志", "Log"))
                .style(Style::default().fg(MUTED)),
        ),
        log_area,
    );
    render_diagnostic_status(frame, status_area, &state.common.status, model.language);
}

fn render_trace(area: Rect, frame: &mut Frame, model: &AppModel) {
    let state = &model.diagnostics.trace;
    let status_area = bottom_row(area);
    let table_area = Rect::new(
        area.x,
        area.y,
        area.width,
        status_area.y.saturating_sub(area.y),
    );
    let hops = state.hops.iter().enumerate().map(|(index, hop)| {
        Row::new(vec![
            Cell::from(format!("{:>2}", hop.ttl)).style(Style::default().fg(SECONDARY)),
            Cell::from(hop.address.clone().unwrap_or_else(|| "*".into())),
            Cell::from(
                hop.latency_ms
                    .map_or_else(|| "*".into(), |value| format!("{value} ms")),
            )
            .style(Style::default().fg(PRIMARY)),
            Cell::from(hop.hostname.clone().unwrap_or_else(|| "-".into()))
                .style(Style::default().fg(SUBTLE)),
        ])
        .style(if index == state.selected {
            Style::default().bg(SELECTED)
        } else {
            Style::default()
        })
    });
    frame.render_widget(
        Table::new(
            hops,
            [
                Constraint::Length(4),
                Constraint::Length(17),
                Constraint::Length(10),
                Constraint::Min(0),
            ],
        )
        .header(
            Row::new([
                tr(model.language, "跳数", "Hop"),
                tr(model.language, "地址", "Address"),
                "RTT",
                tr(model.language, "主机", "Host"),
            ])
            .style(Style::default().fg(MUTED)),
        ),
        table_area,
    );
    render_diagnostic_status(frame, status_area, &state.common.status, model.language);
}

fn render_port_scan(area: Rect, frame: &mut Frame, model: &AppModel) {
    let state = &model.diagnostics.port_scan;
    let stats_area = Rect::new(area.x, area.y, area.width, area.height.min(2));
    let status_area = bottom_row(area);
    let progress_area = Rect::new(area.x, status_area.y.saturating_sub(1), area.width, 1);
    let ports_area = Rect::new(
        area.x,
        stats_area.bottom(),
        area.width,
        progress_area.y.saturating_sub(stats_area.bottom()),
    );
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(
                format!("{}: ", tr(model.language, "开放端口", "Open")),
                Style::default().fg(MUTED),
            ),
            Span::styled(
                format!("{:<6}", state.open_ports.len()),
                Style::default().fg(PRIMARY).add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!("{}: ", tr(model.language, "已扫描", "Scanned")),
                Style::default().fg(MUTED),
            ),
            Span::styled(
                format!("{} / {}", state.scanned, state.total),
                Style::default().fg(SECONDARY),
            ),
        ])),
        stats_area,
    );
    let ports = state.open_ports.iter().map(|port| {
        Row::new(vec![
            Cell::from(port.to_string()),
            Cell::from(well_known_service(*port)).style(Style::default().fg(SECONDARY)),
        ])
    });
    frame.render_widget(
        Table::new(ports, [Constraint::Length(10), Constraint::Min(0)])
            .header(
                Row::new([
                    tr(model.language, "端口", "Port"),
                    tr(model.language, "服务", "Service"),
                ])
                .style(Style::default().fg(MUTED)),
            )
            .block(Block::default().borders(Borders::TOP).title(tr(
                model.language,
                "开放端口",
                "Open Ports",
            ))),
        ports_area,
    );
    let ratio = if state.total == 0 {
        0.0
    } else {
        state.scanned as f64 / state.total as f64
    };
    frame.render_widget(
        Gauge::default()
            .gauge_style(Style::default().fg(SECONDARY).bg(SELECTED))
            .ratio(ratio.clamp(0.0, 1.0))
            .label(format!("{:.0}%", ratio * 100.0)),
        progress_area,
    );
    render_diagnostic_status(frame, status_area, &state.common.status, model.language);
}

fn render_diagnostic_status(
    frame: &mut Frame,
    area: Rect,
    status: &TaskStatus,
    language: Language,
) {
    let (text, color) = match status {
        TaskStatus::Running => (
            format!(
                "{} | {}",
                tr(language, "运行中", "Running"),
                tr(language, "Space 停止", "Space to stop")
            ),
            Color::Green,
        ),
        TaskStatus::Done => (
            tr(language, "完成 | Space 重新开始", "Done | Space to restart").into(),
            SECONDARY,
        ),
        TaskStatus::Failed(error) => (
            format!("{}: {error}", tr(language, "失败", "Failed")),
            Color::Red,
        ),
        TaskStatus::Idle => (
            format!(
                "{} | {}",
                tr(language, "已停止", "Stopped"),
                tr(language, "Space 开始", "Space to start")
            ),
            Color::Red,
        ),
    };
    frame.render_widget(Paragraph::new(text).style(Style::default().fg(color)), area);
}

fn bottom_row(area: Rect) -> Rect {
    Rect::new(
        area.x,
        area.bottom().saturating_sub(1),
        area.width,
        area.height.min(1),
    )
}

fn well_known_service(port: u16) -> &'static str {
    match port {
        20 | 21 => "FTP",
        22 => "SSH",
        23 => "Telnet",
        25 => "SMTP",
        53 => "DNS",
        67 | 68 => "DHCP",
        80 => "HTTP",
        110 => "POP3",
        123 => "NTP",
        143 => "IMAP",
        443 => "HTTPS",
        445 => "SMB",
        3306 => "MySQL",
        3389 => "RDP",
        5432 => "PostgreSQL",
        6379 => "Redis",
        8080 => "HTTP-alt",
        _ => "-",
    }
}

fn render_public_speed(area: Rect, frame: &mut Frame, model: &AppModel) {
    let state = &model.diagnostics.public_speed;
    let latest = state.samples.last();
    let current = latest.map_or(0, |sample| sample.bytes_per_second);
    let total = latest
        .map(|sample| sample.bytes)
        .or_else(|| state.summary.as_ref().map(|summary| summary.total_bytes))
        .unwrap_or_default();
    let elapsed = latest.map_or(0, |sample| sample.elapsed_ms);
    let average = state
        .summary
        .as_ref()
        .map_or(0, |summary| summary.average_bytes_per_second);
    let peak = state.summary.as_ref().map_or(current, |summary| {
        summary.peak_bytes_per_second.max(current)
    });
    let status_area = bottom_row(area);
    let metric_height = status_area.y.saturating_sub(area.y).min(6);
    let metric_area = Rect::new(area.x, area.y, area.width, metric_height);
    let metrics = Layout::vertical([
        Constraint::Length(metric_height.min(2)),
        Constraint::Length(metric_height.saturating_sub(2).min(2)),
        Constraint::Length(metric_height.saturating_sub(4).min(2)),
    ])
    .split(metric_area);
    let chart_area = Rect::new(
        area.x,
        metric_area.bottom(),
        area.width,
        status_area.y.saturating_sub(metric_area.bottom()),
    );
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(
                tr(model.language, "当前速率  ", "Current speed  "),
                Style::default().fg(MUTED),
            ),
            Span::styled(
                format_speed_dual(current),
                Style::default().fg(PRIMARY).add_modifier(Modifier::BOLD),
            ),
        ])),
        metrics[0],
    );
    frame.render_widget(
        Paragraph::new(format!(
            "{}: {}    {}: {}",
            tr(model.language, "平均", "Average"),
            format_speed_dual(average),
            tr(model.language, "峰值", "Peak"),
            format_speed_dual(peak)
        )),
        metrics[1],
    );
    frame.render_widget(
        Paragraph::new(format!(
            "{}: {}    {}: {:.1}s    {}: {}",
            tr(model.language, "已下载", "Downloaded"),
            format_bytes(total),
            tr(model.language, "用时", "Elapsed"),
            elapsed as f64 / 1_000.0,
            tr(model.language, "状态", "Status"),
            task_label(&state.common.status, model.language).trim()
        )),
        metrics[2],
    );
    let history = state
        .samples
        .iter()
        .map(|sample| sample.bytes_per_second)
        .collect::<Vec<_>>();
    frame.render_widget(
        Sparkline::default()
            .block(Block::default().borders(Borders::TOP).title(tr(
                model.language,
                " 速率历史 ",
                " Speed history ",
            )))
            .data(&history)
            .style(Style::default().fg(PRIMARY)),
        chart_area,
    );
    render_diagnostic_status(frame, status_area, &state.common.status, model.language);
}

fn render_link_quality(area: Rect, frame: &mut Frame, model: &AppModel) {
    let state = &model.diagnostics.link_quality;
    let is_wifi = snapshot_is_wifi(state);
    let dimension_count = if is_wifi { 6 } else { 3 };
    let metric_count = if is_wifi { 6 } else { 4 };
    let status_area = bottom_row(area);
    let rssi_height = if is_wifi {
        3.min(status_area.y.saturating_sub(area.y))
    } else {
        0
    };
    let rssi_area = Rect::new(
        area.x,
        status_area.y.saturating_sub(rssi_height),
        area.width,
        rssi_height,
    );
    let header_area = Rect::new(area.x, area.y, area.width, area.height.min(1));
    let overall_area = Rect::new(
        area.x,
        header_area.bottom(),
        area.width,
        area.height.saturating_sub(1).min(1),
    );
    let dimensions_area = Rect::new(area.x, overall_area.bottom(), area.width, dimension_count);
    let metrics_area = Rect::new(area.x, dimensions_area.bottom(), area.width, metric_count);
    let history_area = Rect::new(
        area.x,
        metrics_area.bottom(),
        area.width,
        rssi_area.y.saturating_sub(metrics_area.bottom()),
    );

    let snapshot = state.snapshot.as_ref();
    let adapter_name = snapshot
        .map(|value| value.adapter.name.as_str())
        .or_else(|| {
            state
                .adapters
                .get(state.selected_adapter)
                .map(|value| value.name.as_str())
        })
        .unwrap_or(tr(model.language, "无可用网卡", "No adapter"));
    let mut header = vec![
        Span::styled(
            format!("{}: ", tr(model.language, "网卡", "Adapter")),
            Style::default().fg(MUTED),
        ),
        Span::styled(adapter_name.to_string(), Style::default().fg(SECONDARY)),
    ];
    if let Some(snapshot) = snapshot {
        header.push(Span::styled(
            format!(
                " [{}] · {}",
                if snapshot.adapter.is_wifi {
                    tr(model.language, "无线", "Wireless")
                } else {
                    tr(model.language, "有线", "Wired")
                },
                snapshot.adapter.ipv4
            ),
            Style::default().fg(SECONDARY),
        ));
        if let Some(wireless) = &snapshot.wireless {
            header.push(Span::styled(
                format!("  SSID: {}", wireless.ssid),
                Style::default().fg(Color::White),
            ));
        }
    }
    frame.render_widget(Paragraph::new(Line::from(header)), header_area);

    if let Some(summary) = &state.summary {
        frame.render_widget(
            Gauge::default()
                .gauge_style(
                    Style::default()
                        .fg(link_grade_color(summary.grade))
                        .bg(SELECTED),
                )
                .ratio((summary.score / 100.0).clamp(0.0, 1.0))
                .label(format!(
                    "{}: {} ({:.0})",
                    tr(model.language, "评级", "Grade"),
                    link_grade(summary.grade, model.language),
                    summary.score
                )),
            overall_area,
        );
        let dimensions = summary
            .dimensions
            .iter()
            .take(dimension_count as usize)
            .map(|dimension| {
                let color = link_score_color(dimension.score);
                Line::from(vec![
                    Span::styled(
                        pad_display(link_dimension(dimension.kind, model.language), 8),
                        Style::default().fg(MUTED),
                    ),
                    Span::styled(score_bar(dimension.score, 12), Style::default().fg(color)),
                    Span::styled(
                        format!(
                            " {:>3.0}{}",
                            dimension.score,
                            if summary.weakest == Some(dimension.kind) {
                                " ◀"
                            } else {
                                ""
                            }
                        ),
                        Style::default().fg(color),
                    ),
                ])
            })
            .collect::<Vec<_>>();
        frame.render_widget(Paragraph::new(dimensions), dimensions_area);

        let min = state
            .samples
            .last()
            .and_then(|sample| sample.min_latency_ms);
        let max = state
            .samples
            .last()
            .and_then(|sample| sample.max_latency_ms);
        let mut metrics = vec![
            Line::from(format!(
                "{}: {}/{}/{} ms   {}: {} ms",
                tr(model.language, "最小/平均/最大", "Min/avg/max"),
                format_optional_u64(min),
                format_optional_f64(summary.average_latency_ms),
                format_optional_u64(max),
                tr(model.language, "抖动", "Jitter"),
                format_optional_f64(summary.jitter_ms)
            )),
            Line::from(format!(
                "{}: {:.1}%   {}: {}/{}",
                tr(model.language, "丢包", "Loss"),
                summary.loss_percent,
                tr(model.language, "收发", "Received"),
                summary.received,
                summary.sent
            )),
        ];
        if let Some(wireless) = snapshot.and_then(|value| value.wireless.as_ref()) {
            let sample = state.samples.last();
            metrics.extend([
                Line::from(format!(
                    "RSSI: {}/{}/{} dBm   {}: {} ({}, {} MHz)",
                    sample
                        .and_then(|value| value.min_rssi_dbm)
                        .map_or_else(|| "—".into(), |value| value.to_string()),
                    format_optional_f64(summary.average_rssi_dbm),
                    sample
                        .and_then(|value| value.max_rssi_dbm)
                        .map_or_else(|| "—".into(), |value| value.to_string()),
                    tr(model.language, "信道", "Channel"),
                    wireless.channel,
                    wireless.band,
                    wireless.frequency_mhz
                )),
                Line::from(format!(
                    "{}: {}%   {}: {}",
                    tr(model.language, "信号质量", "Signal quality"),
                    format_optional_f64(summary.average_signal_quality),
                    tr(model.language, "制式", "PHY"),
                    wireless.phy_type
                )),
                Line::from(format!(
                    "Tx/Rx: {}/{} Mbps",
                    wireless.tx_rate_mbps, wireless.rx_rate_mbps
                )),
                Line::from(format!(
                    "BSSID: {}   {} / {}",
                    wireless.bssid, wireless.authentication, wireless.cipher
                )),
            ]);
        } else if let Some(snapshot) = snapshot {
            metrics.extend([
                Line::from(format!(
                    "{}: {}",
                    tr(model.language, "链路速率", "Link speed"),
                    summary
                        .link_speed_bps
                        .map(|speed| format_speed_dual(speed / 8))
                        .unwrap_or_else(|| "—".into())
                )),
                Line::from(format!(
                    "MAC: {}   IPv4: {}",
                    snapshot.adapter.mac, snapshot.adapter.ipv4
                )),
            ]);
        }
        frame.render_widget(Paragraph::new(metrics), metrics_area);
    } else {
        frame.render_widget(
            Paragraph::new(tr(
                model.language,
                "等待链路质量样本…",
                "Waiting for link-quality samples…",
            ))
            .style(Style::default().fg(SUBTLE)),
            dimensions_area,
        );
    }

    let latency = state
        .samples
        .iter()
        .map(|sample| sample.latency_ms.unwrap_or_default())
        .collect::<Vec<_>>();
    frame.render_widget(
        Sparkline::default()
            .block(Block::default().borders(Borders::TOP).title(tr(
                model.language,
                "延迟历史",
                "Latency History",
            )))
            .data(&latency)
            .style(Style::default().fg(PRIMARY)),
        history_area,
    );
    if is_wifi {
        let rssi = state
            .samples
            .iter()
            .map(|sample| {
                sample
                    .rssi_dbm
                    .map_or(0, |value| (value + 100).max(0) as u64)
            })
            .collect::<Vec<_>>();
        frame.render_widget(
            Sparkline::default()
                .block(Block::default().borders(Borders::TOP).title(tr(
                    model.language,
                    "RSSI 历史",
                    "RSSI History",
                )))
                .data(&rssi)
                .style(Style::default().fg(Color::Magenta)),
            rssi_area,
        );
    }
    render_diagnostic_status(frame, status_area, &state.common.status, model.language);
}

fn snapshot_is_wifi(state: &iptools_core::LinkQualityState) -> bool {
    state
        .snapshot
        .as_ref()
        .is_some_and(|snapshot| snapshot.adapter.is_wifi)
}

fn format_speed_dual(bytes_per_second: u64) -> String {
    if bytes_per_second >= 1_000_000 {
        format!(
            "{:.2} MB/s · {:.2} Mbps",
            bytes_per_second as f64 / 1_000_000.0,
            bytes_per_second as f64 * 8.0 / 1_000_000.0
        )
    } else {
        format!(
            "{:.1} KB/s · {:.2} Mbps",
            bytes_per_second as f64 / 1_000.0,
            bytes_per_second as f64 * 8.0 / 1_000_000.0
        )
    }
}

fn format_optional_f64(value: Option<f64>) -> String {
    value.map_or_else(|| "—".into(), |value| format!("{value:.1}"))
}

fn format_optional_u64(value: Option<u64>) -> String {
    value.map_or_else(|| "—".into(), |value| value.to_string())
}

fn pad_display(value: &str, width: usize) -> String {
    let padding = width.saturating_sub(UnicodeWidthStr::width(value));
    format!("{value}{}", " ".repeat(padding))
}

fn link_grade(grade: LinkQualityGrade, language: Language) -> &'static str {
    match grade {
        LinkQualityGrade::Excellent => tr(language, "优秀", "Excellent"),
        LinkQualityGrade::Good => tr(language, "良好", "Good"),
        LinkQualityGrade::Fair => tr(language, "一般", "Fair"),
        LinkQualityGrade::Poor => tr(language, "较差", "Poor"),
    }
}

fn link_grade_color(grade: LinkQualityGrade) -> Color {
    match grade {
        LinkQualityGrade::Excellent => Color::Green,
        LinkQualityGrade::Good => Color::Cyan,
        LinkQualityGrade::Fair => Color::Yellow,
        LinkQualityGrade::Poor => Color::Red,
    }
}

fn link_score_color(score: f64) -> Color {
    if score >= 85.0 {
        Color::Green
    } else if score >= 70.0 {
        Color::Cyan
    } else if score >= 50.0 {
        Color::Yellow
    } else {
        Color::Red
    }
}

fn link_dimension(kind: LinkQualityDimensionKind, language: Language) -> &'static str {
    match kind {
        LinkQualityDimensionKind::Loss => tr(language, "丢包", "Loss"),
        LinkQualityDimensionKind::Latency => tr(language, "延迟", "Latency"),
        LinkQualityDimensionKind::Jitter => tr(language, "抖动", "Jitter"),
        LinkQualityDimensionKind::Signal => tr(language, "信号", "Signal"),
        LinkQualityDimensionKind::Rate => tr(language, "速率", "Rate"),
        LinkQualityDimensionKind::Phy => tr(language, "制式", "PHY"),
    }
}

fn score_bar(score: f64, width: usize) -> String {
    let filled = ((score / 100.0) * width as f64).round() as usize;
    let filled = filled.min(width);
    format!("{}{}", "█".repeat(filled), "░".repeat(width - filled))
}

fn diagnostic_target_index(tool: DiagnosticTool) -> usize {
    if tool == DiagnosticTool::LinkQuality {
        1
    } else {
        0
    }
}

fn diagnostic_failure(
    common: &iptools_core::DiagnosticCommonState,
    language: Language,
) -> Option<String> {
    let TaskStatus::Failed(status_message) = &common.status else {
        return None;
    };
    let label = match common.error.as_ref().map(|error| error.code) {
        Some(RuntimeErrorCode::InvalidRequest) => tr(language, "参数无效", "Invalid request"),
        Some(RuntimeErrorCode::ResolveTarget) => {
            tr(language, "无法解析目标", "Target resolution failed")
        }
        Some(RuntimeErrorCode::PermissionDenied) => tr(language, "权限不足", "Permission denied"),
        Some(RuntimeErrorCode::Timeout) => tr(language, "请求超时", "Request timed out"),
        Some(RuntimeErrorCode::Network) => tr(language, "网络错误", "Network error"),
        Some(RuntimeErrorCode::Cancelled) => tr(language, "任务已取消", "Task cancelled"),
        Some(RuntimeErrorCode::Internal) => tr(language, "内部错误", "Internal error"),
        None => tr(language, "执行失败", "Operation failed"),
    };
    let detail = common
        .error
        .as_ref()
        .map(|error| error.message.as_str())
        .unwrap_or(status_message);
    if detail.is_empty() || detail.starts_with("diag_") {
        Some(label.into())
    } else {
        Some(format!("{label}: {detail}"))
    }
}

fn active_diagnostic_config_index(model: &AppModel) -> usize {
    match model.diagnostics.tool {
        DiagnosticTool::Ping => model.diagnostics.ping.config_selected,
        DiagnosticTool::Trace => model.diagnostics.trace.config_selected,
        DiagnosticTool::LinkQuality => model.diagnostics.link_quality.config_selected,
        _ => 0,
    }
}

fn diagnostic_fields(model: &AppModel) -> Vec<(&'static str, String)> {
    match model.diagnostics.tool {
        DiagnosticTool::Ping => vec![
            (
                tr(model.language, "目标", "Target"),
                model.diagnostics.ping.request.target.clone(),
            ),
            (
                tr(model.language, "间隔", "Interval"),
                format!("{} ms", model.diagnostics.ping.request.interval_ms),
            ),
            (
                tr(model.language, "超时", "Timeout"),
                format!("{} ms", model.diagnostics.ping.request.timeout_ms),
            ),
            (
                tr(model.language, "载荷", "Payload"),
                format!("{} B", model.diagnostics.ping.request.packet_size),
            ),
        ],
        DiagnosticTool::Trace => vec![
            (
                tr(model.language, "目标", "Target"),
                model.diagnostics.trace.request.target.clone(),
            ),
            (
                tr(model.language, "最大跳数", "Max hops"),
                model.diagnostics.trace.max_hops_input.clone(),
            ),
            (
                tr(model.language, "超时", "Timeout"),
                model.diagnostics.trace.timeout_input.clone(),
            ),
        ],
        DiagnosticTool::PublicSpeed => vec![(
            tr(model.language, "服务器", "Server"),
            model
                .diagnostics
                .public_speed
                .server
                .clone()
                .unwrap_or_else(|| tr(model.language, "自动选择", "Automatic").into()),
        )],
        DiagnosticTool::LinkQuality => {
            let state = &model.diagnostics.link_quality;
            let adapter = state
                .adapters
                .get(state.selected_adapter)
                .map(|adapter| format!("{} ({})", adapter.name, adapter.ipv4))
                .unwrap_or_else(|| tr(model.language, "无可用网卡", "No adapter").into());
            vec![
                (tr(model.language, "网卡", "Adapter"), adapter),
                (
                    tr(model.language, "目标", "Target"),
                    state.params.target.clone(),
                ),
                (
                    tr(model.language, "次数", "Count"),
                    state.params.count.clone(),
                ),
                (
                    tr(model.language, "间隔", "Interval"),
                    state.params.interval_ms.clone(),
                ),
                (
                    tr(model.language, "超时", "Timeout"),
                    state.params.timeout_ms.clone(),
                ),
                (
                    tr(model.language, "载荷", "Payload"),
                    state.params.packet_size.clone(),
                ),
            ]
        }
        _ => vec![(
            tr(model.language, "目标", "Target"),
            model.diagnostics.active_target().to_string(),
        )],
    }
}

fn render_settings(frame: &mut Frame, area: Rect, model: &AppModel, ui: &mut UiState) {
    let language = match model.language {
        Language::Zh => "简体中文",
        Language::En => "English",
    };
    let rows = Layout::vertical([Constraint::Min(0), Constraint::Length(3)]).split(area);
    let list_inner = Block::bordered().inner(rows[0]);
    for index in 0..3 {
        ui.settings_regions.push((
            Rect::new(list_inner.x, list_inner.y + index, list_inner.width, 1),
            index as usize,
        ));
    }
    let values = [
        (tr(model.language, "语言", "Language"), language.to_string()),
        (
            tr(model.language, "扫描并发数", "Scan concurrency"),
            model.scan_concurrency.to_string(),
        ),
        (
            tr(
                model.language,
                "清空参数记忆",
                "Reset remembered parameters",
            ),
            if model.settings_just_reset {
                tr(model.language, "已清空 ✓", "Cleared ✓").to_string()
            } else {
                tr(model.language, "按 Enter 清空", "Press Enter to clear").to_string()
            },
        ),
    ];
    let items = values
        .into_iter()
        .enumerate()
        .map(|(index, (label, value))| {
            let selected = index == model.settings_selected;
            ListItem::new(Line::from(vec![
                Span::styled(
                    if selected { "> " } else { "  " },
                    Style::default().fg(PRIMARY),
                ),
                Span::styled(pad_display(label, 20), Style::default().fg(MUTED)),
                Span::raw(" : "),
                Span::styled(
                    value,
                    Style::default().fg(SECONDARY).add_modifier(Modifier::BOLD),
                ),
            ]))
            .style(if selected {
                Style::default().bg(SELECTED)
            } else {
                Style::default()
            })
        });
    frame.render_widget(
        List::new(items).block(Block::bordered().title(tr(model.language, " 设置 ", " Settings "))),
        rows[0],
    );
    frame.render_widget(
        Paragraph::new(tr(
            model.language,
            "方向键选择与修改；Enter 确认。清空仅重置参数记忆，不改变当前页面。",
            "Use arrows to select and edit; Enter confirms. Reset keeps the current page.",
        ))
        .block(Block::bordered())
        .style(Style::default().fg(Color::Yellow))
        .alignment(Alignment::Center),
        rows[1],
    );
}

fn render_footer(frame: &mut Frame, area: Rect, model: &AppModel) {
    frame.render_widget(
        Paragraph::new(tr(
            model.language,
            " [Tab/Shift+Tab] 切换   [Ctrl+L] 语言   [F1] 帮助   [Ctrl+C] 退出 ",
            " [Tab/Shift+Tab] Switch   [Ctrl+L] Language   [F1] Help   [Ctrl+C] Quit ",
        ))
        .style(Style::default().fg(MUTED))
        .alignment(Alignment::Left),
        area,
    );
}

fn render_help(frame: &mut Frame, language: Language) {
    let area = centered(frame.area(), 66, 72);
    frame.render_widget(Clear, area);
    frame.render_widget(
        Paragraph::new(tr(
            language,
            "键盘与触控快捷键\n\nTab / Shift+Tab  切换页面\n方向键 / WASD    导航\nEnter / Space     开始或停止\nE                 编辑\nCtrl+L            切换语言\nF1 / Esc          打开或关闭帮助\n\n在线版本使用确定性模拟数据。",
            "Keyboard and touch shortcuts\n\nTab / Shift+Tab  switch pages\nArrows / WASD     navigate\nEnter / Space     start or stop\nE                 edit\nCtrl+L            toggle language\nF1 / Esc          open or close help\n\nThe online version uses deterministic simulated data.",
        ))
        .block(Block::bordered().title(tr(language, " 帮助 ", " Help ")))
        .alignment(Alignment::Left)
        .wrap(Wrap { trim: true }),
        area,
    );
}

fn centered(area: Rect, width: u16, height: u16) -> Rect {
    let vertical = Layout::vertical([
        Constraint::Percentage((100 - height) / 2),
        Constraint::Percentage(height),
        Constraint::Percentage((100 - height) / 2),
    ])
    .split(area);
    Layout::horizontal([
        Constraint::Percentage((100 - width) / 2),
        Constraint::Percentage(width),
        Constraint::Percentage((100 - width) / 2),
    ])
    .split(vertical[1])[1]
}

fn contains(area: Rect, x: u16, y: u16) -> bool {
    x >= area.x && x < area.right() && y >= area.y && y < area.bottom()
}

fn tr<'a>(language: Language, zh: &'a str, en: &'a str) -> &'a str {
    match language {
        Language::Zh => zh,
        Language::En => en,
    }
}

fn page_label(page: Page, language: Language) -> &'static str {
    match (page, language) {
        (Page::Dashboard, Language::Zh) => "概览",
        (Page::Adapters, Language::Zh) => "适配器",
        (Page::Scanner, Language::Zh) => "扫描",
        (Page::Traffic, Language::Zh) => "流量",
        (Page::Diagnostics, Language::Zh) => "诊断",
        (Page::Settings, Language::Zh) => "设置",
        (Page::Dashboard, Language::En) => "Dashboard",
        (Page::Adapters, Language::En) => "Adapters",
        (Page::Scanner, Language::En) => "Scanner",
        (Page::Traffic, Language::En) => "Traffic",
        (Page::Diagnostics, Language::En) => "Diagnostics",
        (Page::Settings, Language::En) => "Settings",
    }
}

fn tool_label(tool: DiagnosticTool, language: Language) -> &'static str {
    match (tool, language) {
        (DiagnosticTool::Ping, _) => "Ping",
        (DiagnosticTool::Trace, Language::Zh) => "路由追踪",
        (DiagnosticTool::PortScan, Language::Zh) => "端口扫描",
        (DiagnosticTool::PublicSpeed, Language::Zh) => "公网测速",
        (DiagnosticTool::LinkQuality, Language::Zh) => "链路质量",
        (DiagnosticTool::LanSpeed, Language::Zh) => "局域网测速",
        (DiagnosticTool::Trace, Language::En) => "Trace Route",
        (DiagnosticTool::PortScan, Language::En) => "Port Scan",
        (DiagnosticTool::PublicSpeed, Language::En) => "Public Speed",
        (DiagnosticTool::LinkQuality, Language::En) => "Link Quality",
        (DiagnosticTool::LanSpeed, Language::En) => "LAN Speed",
    }
}

fn task_label(status: &TaskStatus, language: Language) -> &'static str {
    match status {
        TaskStatus::Idle => tr(language, " 空闲 ", " Idle "),
        TaskStatus::Running => tr(language, " 运行中 · 点击停止 ", " Running · click to stop "),
        TaskStatus::Done => tr(
            language,
            " 完成 · 点击重新开始 ",
            " Done · click to restart ",
        ),
        TaskStatus::Failed(_) => tr(language, " 失败 · 点击重试 ", " Failed · click to retry "),
    }
}

fn format_rate(value: u64) -> String {
    format!("{}/s", format_bytes(value))
}

fn format_bytes(value: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KiB", "MiB", "GiB", "TiB"];
    let mut amount = value as f64;
    let mut unit = 0;
    while amount >= 1024.0 && unit < UNITS.len() - 1 {
        amount /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{} {}", value, UNITS[unit])
    } else {
        format!("{amount:.1} {}", UNITS[unit])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::{Terminal, backend::TestBackend};

    #[test]
    fn renders_demo_banner_and_hit_regions() {
        let backend = TestBackend::new(120, 36);
        let mut terminal = Terminal::new(backend).unwrap();
        let model = AppModel::default();
        let mut ui = UiState::default();
        terminal
            .draw(|frame| render(frame, &model, &mut ui))
            .unwrap();
        let text = terminal.backend().to_string();
        assert!(text.contains("DEMO"));
        assert_eq!(ui.hit_test(2, 1), Some(Action::SelectPage(0)));
    }

    #[test]
    fn dashboard_states_render_in_both_languages_and_compact_sizes() {
        for (width, height) in [(80, 24), (120, 36)] {
            for language in [Language::En, Language::Zh] {
                for status in [
                    TaskStatus::Idle,
                    TaskStatus::Running,
                    TaskStatus::Done,
                    TaskStatus::Failed("offline".into()),
                ] {
                    let backend = TestBackend::new(width, height);
                    let mut terminal = Terminal::new(backend).unwrap();
                    let mut model = AppModel::default();
                    model.language = language;
                    model.dashboard.status = status;
                    model.dashboard.snapshot.hostname = "demo-router".into();
                    model.dashboard.snapshot.active_interface =
                        Some(iptools_core::DashboardInterface {
                            name: "Ethernet".into(),
                            description: "Physical adapter".into(),
                            ipv4: "192.168.1.20".into(),
                            ssid: None,
                            is_physical: true,
                            dhcp_enabled: true,
                        });
                    model.dashboard.snapshot.public_info = Some(iptools_core::PublicIpInfo {
                        ip: "203.0.113.10".into(),
                        city: "杭州".into(),
                        region: "浙江".into(),
                        country: "中国".into(),
                        isp: "Example ISP".into(),
                    });
                    let mut ui = UiState::default();

                    terminal
                        .draw(|frame| render(frame, &model, &mut ui))
                        .unwrap();
                    let text = terminal.backend().to_string();
                    assert!(text.contains("demo-router"));
                    assert!(text.contains("203.0.113.10"));
                }
            }
        }
    }

    #[test]
    fn adapter_and_traffic_states_render_in_both_languages_and_compact_sizes() {
        for (width, height) in [(80, 24), (120, 36)] {
            for language in [Language::En, Language::Zh] {
                let mut model = AppModel::default();
                model.language = language;
                model.page = Page::Adapters;
                model.adapters.items = vec![iptools_core::AdapterInfo {
                    name: "Wi-Fi".into(),
                    description: "Wireless LAN adapter".into(),
                    kind: "wireless".into(),
                    ipv4: "192.168.1.20".into(),
                    mac: "02:11:22:33:44:55".into(),
                    status: "up".into(),
                    ssid: Some("实验网络".into()),
                    dhcp_enabled: true,
                    is_physical: true,
                    link_speed_bps: Some(866_000_000),
                    download_bps: 1_048_576,
                    upload_bps: 262_144,
                    total_download: 8_589_934_592,
                    total_upload: 1_610_612_736,
                    ..iptools_core::AdapterInfo::default()
                }];
                model.adapters.status = TaskStatus::Done;
                let backend = TestBackend::new(width, height);
                let mut terminal = Terminal::new(backend).unwrap();
                let mut ui = UiState::default();
                terminal
                    .draw(|frame| render(frame, &model, &mut ui))
                    .unwrap();
                let text = terminal.backend().to_string();
                assert!(text.contains("192.168.1.20"), "{text}");
                assert!(text.contains("Wireless LAN"), "{text}");
                assert_eq!(ui.hit_test(2, 4), Some(Action::SelectAdapter(0)));

                model.page = Page::Traffic;
                model.traffic.rows = vec![iptools_core::TrafficRow {
                    name: "Wi-Fi".into(),
                    download_bps: 1_048_576,
                    upload_bps: 262_144,
                    total_download: 8_589_934_592,
                    total_upload: 1_610_612_736,
                    session_download: 734_003_200,
                    session_upload: 125_829_120,
                }];
                model.traffic.status = TaskStatus::Running;
                terminal
                    .draw(|frame| render(frame, &model, &mut ui))
                    .unwrap();
                let text = terminal.backend().to_string();
                assert!(text.contains("Wi-Fi"));
                assert!(text.contains("1.0 MiB/s"));
            }
        }
    }

    #[test]
    fn adapter_edit_phases_render_and_mouse_fields_use_fixed_display_columns() {
        for (width, height) in [(80, 24), (120, 36)] {
            for language in [Language::En, Language::Zh] {
                let backend = TestBackend::new(width, height);
                let mut terminal = Terminal::new(backend).unwrap();
                let mut model = AppModel::default();
                model.page = Page::Adapters;
                model.language = language;
                model.adapters.items.push(iptools_core::AdapterInfo {
                    name: "Ethernet".into(),
                    guid: "demo-ethernet".into(),
                    ipv4: "192.168.1.20".into(),
                    cidr: Some("192.168.1.20/24".into()),
                    dhcp_enabled: false,
                    ..iptools_core::AdapterInfo::default()
                });
                model.update(iptools_core::Message::Input(
                    iptools_core::InputEvent::Action(Action::Edit),
                ));
                let mut ui = UiState::default();
                for phase in [
                    AdapterEditPhase::Editing,
                    AdapterEditPhase::Confirming,
                    AdapterEditPhase::Applying,
                    AdapterEditPhase::Succeeded(AdapterApplyOutcome::RuntimeOnly),
                    AdapterEditPhase::Failed(iptools_core::RuntimeError::new(
                        iptools_core::RuntimeErrorCode::PermissionDenied,
                        "administrator required",
                    )),
                ] {
                    let editing = phase == AdapterEditPhase::Editing;
                    model.adapters.edit.as_mut().unwrap().phase = phase;
                    terminal
                        .draw(|frame| render(frame, &model, &mut ui))
                        .unwrap();
                    let text = terminal.backend().to_string();
                    assert!(text.contains("192.168.1.20"), "{text}");
                    assert!(text.contains("255.255.255.0"), "{text}");
                    if editing {
                        assert_eq!(
                            ui.hit_test(20, 5),
                            Some(Action::SelectAdapterField(AdapterField::Ipv4, 1))
                        );
                    } else {
                        assert_eq!(ui.hit_test(20, 5), None);
                    }
                }
            }
        }
    }

    #[test]
    fn scanner_states_render_in_both_languages_and_compact_sizes() {
        for (width, height) in [(80, 24), (120, 36)] {
            for language in [Language::En, Language::Zh] {
                for status in [TaskStatus::Idle, TaskStatus::Running, TaskStatus::Done] {
                    let backend = TestBackend::new(width, height);
                    let mut terminal = Terminal::new(backend).unwrap();
                    let mut model = AppModel::default();
                    model.page = Page::Scanner;
                    model.language = language;
                    model.scanner.status = status;
                    model.scanner.current = 42;
                    model.scanner.total = 254;
                    model.scanner.results = vec![iptools_core::ScanHost {
                        ip: "192.168.1.1".into(),
                        mac: "00:11:22:33:44:55".into(),
                        vendor: "Example Networks".into(),
                        hostname: "gateway".into(),
                    }];
                    let mut ui = UiState::default();

                    terminal
                        .draw(|frame| render(frame, &model, &mut ui))
                        .unwrap();
                    let text = terminal.backend().to_string();
                    assert!(text.contains("192.168.1.0/24"));
                    assert!(text.contains("192.168.1.1"));
                    assert!(ui.scanner_action.is_some());
                }
            }
        }
    }

    #[test]
    fn port_scan_states_render_in_both_languages_and_compact_sizes() {
        for (width, height) in [(80, 24), (120, 36)] {
            for language in [Language::En, Language::Zh] {
                for status in [
                    TaskStatus::Idle,
                    TaskStatus::Running,
                    TaskStatus::Done,
                    TaskStatus::Failed("resolve failed".into()),
                ] {
                    let failed = matches!(&status, TaskStatus::Failed(_));
                    let backend = TestBackend::new(width, height);
                    let mut terminal = Terminal::new(backend).unwrap();
                    let mut model = AppModel::default();
                    model.page = Page::Diagnostics;
                    model.language = language;
                    model.diagnostics.tool = DiagnosticTool::PortScan;
                    model.diagnostics.port_scan.request.target = "192.0.2.10".into();
                    model.diagnostics.port_scan.common.status = status;
                    model.diagnostics.port_scan.common.progress = 75;
                    model.diagnostics.port_scan.common.primary = "open: 443".into();
                    model.diagnostics.port_scan.common.log = vec!["open: 22".into()];
                    model.diagnostics.port_scan.open_ports = vec![22, 443];
                    let mut ui = UiState::default();

                    terminal
                        .draw(|frame| render(frame, &model, &mut ui))
                        .unwrap();
                    let text = terminal.backend().to_string();
                    assert!(text.contains("192.0.2.10"));
                    if failed {
                        assert!(text.contains(if language == Language::Zh {
                            "执行失败"
                        } else {
                            "Operation failed"
                        }));
                    } else {
                        assert!(text.contains("443"));
                        assert!(text.contains("HTTPS"));
                    }
                    assert!(ui.diagnostic_main.is_some());
                }
            }
        }
    }

    #[test]
    fn ping_and_trace_shared_focus_config_and_results_render_in_both_languages() {
        for language in [Language::En, Language::Zh] {
            let backend = TestBackend::new(120, 36);
            let mut terminal = Terminal::new(backend).unwrap();
            let mut model = AppModel::default();
            model.page = Page::Diagnostics;
            model.language = language;
            model.diagnostics.focused = true;
            model.diagnostics.focus = DiagnosticFocus::Config;
            model.diagnostics.ping.common.status = TaskStatus::Running;
            model.diagnostics.ping.common.primary = "reply 4: 18 ms".into();
            model.diagnostics.ping.common.detail = "4 / 4 received".into();
            model.diagnostics.ping.common.log = vec!["reply 3: 20 ms".into()];
            let mut ui = UiState::default();
            terminal
                .draw(|frame| render(frame, &model, &mut ui))
                .unwrap();
            let text = terminal.backend().to_string();
            assert!(text.contains("8.8.8.8"), "{text}");
            assert!(text.contains("1000 ms"), "{text}");
            assert!(text.contains("reply 3: 20 ms"), "{text}");
            assert!(
                text.contains(if language == Language::Zh {
                    "运行中"
                } else {
                    "Running"
                }),
                "{text}"
            );
            assert_eq!(
                ui.hit_test(89, 5),
                Some(Action::SelectDiagnosticField(0, 1))
            );
            assert_eq!(
                ui.hit_test(25, 5),
                Some(Action::FocusDiagnostic(DiagnosticFocus::Main))
            );

            model.diagnostics.tool = DiagnosticTool::Trace;
            model.diagnostics.focus = DiagnosticFocus::Main;
            model.diagnostics.trace.hops = vec![iptools_core::TraceHop {
                ttl: 1,
                address: Some("192.0.2.1".into()),
                hostname: Some("gateway.example".into()),
                latency_ms: Some(7),
            }];
            terminal
                .draw(|frame| render(frame, &model, &mut ui))
                .unwrap();
            let text = terminal.backend().to_string();
            assert!(text.contains("192.0.2.1"), "{text}");
            assert!(text.contains("gateway.example"), "{text}");
            assert!(text.contains("30"), "{text}");

            let error = iptools_core::RuntimeError::new(
                RuntimeErrorCode::PermissionDenied,
                "raw socket permission denied",
            );
            model.diagnostics.trace.common.status = TaskStatus::Failed(error.message.clone());
            model.diagnostics.trace.common.error = Some(error);
            terminal
                .draw(|frame| render(frame, &model, &mut ui))
                .unwrap();
            let text = terminal.backend().to_string();
            assert!(
                text.contains(if language == Language::Zh {
                    "权限不足"
                } else {
                    "Permission denied"
                }),
                "{text}"
            );
        }
    }

    #[test]
    fn public_speed_and_link_quality_render_full_shared_results_in_both_languages() {
        for language in [Language::En, Language::Zh] {
            let backend = TestBackend::new(120, 36);
            let mut terminal = Terminal::new(backend).unwrap();
            let mut model = AppModel::default();
            model.page = Page::Diagnostics;
            model.language = language;
            model.diagnostics.focused = true;
            model.diagnostics.focus = DiagnosticFocus::Main;
            model.diagnostics.tool = DiagnosticTool::PublicSpeed;
            model.diagnostics.public_speed.server = Some("demo.invalid".into());
            model.diagnostics.public_speed.samples = vec![iptools_core::SpeedSample {
                elapsed_ms: 2_000,
                bytes: 16_000_000,
                bytes_per_second: 8_000_000,
            }];
            model.diagnostics.public_speed.summary = Some(iptools_core::SpeedSummary {
                average_bytes_per_second: 7_000_000,
                peak_bytes_per_second: 8_000_000,
                total_bytes: 16_000_000,
            });
            let mut ui = UiState::default();
            terminal
                .draw(|frame| render(frame, &model, &mut ui))
                .unwrap();
            let text = terminal.backend().to_string();
            assert!(text.contains("64.00 Mbps"), "{text}");
            assert!(text.contains("demo.invalid"), "{text}");

            let adapter = iptools_core::LinkQualityAdapter {
                key: "wifi-guid".into(),
                name: "Wi-Fi".into(),
                guid: "wifi-guid".into(),
                ipv4: "192.168.1.21".into(),
                is_wifi: true,
                link_speed_bps: Some(866_000_000),
                mac: "02:00:00:00:00:21".into(),
            };
            let snapshot = iptools_core::LinkQualitySnapshot {
                adapter: adapter.clone(),
                wireless: Some(iptools_core::WirelessSnapshot {
                    ssid: "Lab".into(),
                    bssid: "02:AA:BB:CC:DD:01".into(),
                    signal_quality: 88,
                    rssi_dbm: -55,
                    phy_type: "802.11ax · Wi-Fi 6".into(),
                    wifi_generation: 6,
                    band: "5 GHz".into(),
                    channel: 36,
                    frequency_mhz: 5_180,
                    rx_rate_mbps: 866,
                    tx_rate_mbps: 780,
                    authentication: "WPA2-Personal".into(),
                    cipher: "CCMP (AES)".into(),
                }),
            };
            let sample = iptools_core::LinkQualitySample {
                sequence: 8,
                latency_ms: Some(20),
                sent: 8,
                received: 8,
                min_latency_ms: Some(18),
                average_latency_ms: Some(20.0),
                max_latency_ms: Some(23),
                jitter_ms: Some(2.0),
                loss_percent: 0.0,
                rssi_dbm: Some(-55),
                min_rssi_dbm: Some(-58),
                average_rssi_dbm: Some(-56.0),
                max_rssi_dbm: Some(-54),
                signal_quality: Some(88),
                min_signal_quality: Some(84),
                average_signal_quality: Some(87.0),
                max_signal_quality: Some(90),
                link_speed_bps: None,
            };
            model.diagnostics.tool = DiagnosticTool::LinkQuality;
            model.diagnostics.focus = DiagnosticFocus::Config;
            model.diagnostics.link_quality.adapters = vec![adapter];
            model.diagnostics.link_quality.snapshot = Some(snapshot.clone());
            model.diagnostics.link_quality.summary = Some(
                iptools_core::link_quality::summary_from_sample(&snapshot, &sample),
            );
            terminal
                .draw(|frame| render(frame, &model, &mut ui))
                .unwrap();
            let text = terminal.backend().to_string();
            assert!(text.contains("Lab"), "{text}");
            assert!(text.contains("802.11ax"), "{text}");
            assert!(text.contains(if language == Language::Zh {
                "优秀"
            } else {
                "Excellent"
            }));
            assert!(matches!(
                ui.hit_test(89, 7),
                Some(Action::SelectDiagnosticField(1, _))
            ));
        }
    }

    #[test]
    fn public_speed_and_link_quality_states_render_at_compact_and_standard_sizes() {
        for (width, height) in [(80, 24), (120, 36)] {
            for language in [Language::En, Language::Zh] {
                for tool in [DiagnosticTool::PublicSpeed, DiagnosticTool::LinkQuality] {
                    for status in [
                        TaskStatus::Idle,
                        TaskStatus::Running,
                        TaskStatus::Done,
                        TaskStatus::Failed("offline".into()),
                    ] {
                        let backend = TestBackend::new(width, height);
                        let mut terminal = Terminal::new(backend).unwrap();
                        let mut model = AppModel::default();
                        model.page = Page::Diagnostics;
                        model.language = language;
                        model.diagnostics.tool = tool;
                        model.diagnostics.active_common_mut().status = status;
                        let mut ui = UiState::default();
                        terminal
                            .draw(|frame| render(frame, &model, &mut ui))
                            .unwrap();
                        let text = terminal.backend().to_string();
                        assert!(text.contains(tool_label(tool, language)), "{text}");
                        assert!(ui.diagnostic_main.is_some());
                    }
                }
            }
        }
    }

    #[test]
    fn settings_preserve_v031_list_reset_feedback_and_mouse_rows() {
        for (width, height) in [(80, 24), (120, 36), (160, 48)] {
            for language in [Language::En, Language::Zh] {
                let backend = TestBackend::new(width, height);
                let mut terminal = Terminal::new(backend).unwrap();
                let mut model = AppModel::default();
                model.page = Page::Settings;
                model.language = language;
                model.settings_selected = 2;
                model.settings_just_reset = true;
                model.scan_concurrency = 120;
                let mut ui = UiState::default();
                terminal
                    .draw(|frame| render(frame, &model, &mut ui))
                    .unwrap();
                let text = terminal.backend().to_string();
                assert!(text.contains("120"), "{text}");
                assert!(
                    text.contains(if language == Language::Zh {
                        "已清空"
                    } else {
                        "Cleared"
                    }),
                    "{text}"
                );
                assert_eq!(ui.hit_test(2, 6), Some(Action::SelectSetting(2)));
            }
        }
    }
}
