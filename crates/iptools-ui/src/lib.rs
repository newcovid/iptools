//! Backend-independent Ratatui rendering for iptools.

use iptools_core::{
    Action, AdapterApplyOutcome, AdapterEditPhase, AdapterField, AdapterValidationError, AppModel,
    DiagnosticFocus, DiagnosticTool, Language, Page, RuntimeErrorCode, TaskStatus,
};
use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Cell, Clear, Gauge, List, ListItem, Paragraph, Row, Table, Wrap},
};
use unicode_width::UnicodeWidthStr;

const PRIMARY: Color = Color::Cyan;
const SECONDARY: Color = Color::Magenta;
const MUTED: Color = Color::DarkGray;
const SELECTED: Color = Color::Rgb(24, 52, 72);

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
            Constraint::Min(12),
            Constraint::Length(2),
        ])
        .split(frame.area());

    render_tabs(frame, areas[0], model, ui);
    match model.page {
        Page::Dashboard => render_dashboard(frame, areas[1], model),
        Page::Adapters => render_adapters(frame, areas[1], model, ui),
        Page::Scanner => render_scanner(frame, areas[1], model, ui),
        Page::Traffic => render_traffic(frame, areas[1], model),
        Page::Diagnostics => render_diagnostics(frame, areas[1], model, ui),
        Page::Settings => render_settings(frame, areas[1], model),
    }
    render_footer(frame, areas[2], model);

    if model.show_help {
        render_help(frame, model.language);
    }
}

fn render_tabs(frame: &mut Frame, area: Rect, model: &AppModel, ui: &mut UiState) {
    let block = Block::default().borders(Borders::ALL).title(if model.demo {
        " IP Tools · DEMO "
    } else {
        " IP Tools "
    });
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let mut x = inner.x;
    for page in Page::ALL {
        let label = format!(" {} ", page_label(page, model.language));
        let width = label.width() as u16;
        let tab = Rect::new(x, inner.y, width.min(inner.right().saturating_sub(x)), 1);
        let style = if model.page == page {
            Style::default()
                .fg(Color::White)
                .bg(SELECTED)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(PRIMARY)
        };
        frame.render_widget(Paragraph::new(label).style(style), tab);
        ui.page_regions.push((tab, page));
        x = x.saturating_add(width + 1);
        if x >= inner.right() {
            break;
        }
    }
}

fn render_dashboard(frame: &mut Frame, area: Rect, model: &AppModel) {
    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(area);
    let snapshot = &model.dashboard.snapshot;
    let mut local = vec![
        line(tr(model.language, "主机", "Host"), &snapshot.hostname),
        line(
            tr(model.language, "系统", "System"),
            &format!("{} {}", snapshot.os_name, snapshot.os_version),
        ),
        line(
            tr(model.language, "更新时间", "Updated"),
            &snapshot.observed_at,
        ),
        line(
            tr(model.language, "模式", "Mode"),
            if model.demo {
                tr(model.language, "确定性演示", "Deterministic demo")
            } else {
                tr(model.language, "原生", "Native")
            },
        ),
    ];
    if let Some(interface) = &snapshot.active_interface {
        let name = interface.ssid.as_ref().map_or_else(
            || interface.name.clone(),
            |ssid| format!("{} (SSID: {ssid})", interface.name),
        );
        local.extend([
            line(tr(model.language, "接口", "Interface"), &name),
            line(tr(model.language, "本地 IP", "Local IP"), &interface.ipv4),
            line(
                tr(model.language, "连接", "Connection"),
                &format!(
                    "{} / {}",
                    if interface.is_physical {
                        tr(model.language, "物理", "physical")
                    } else {
                        tr(model.language, "虚拟", "virtual")
                    },
                    if interface.dhcp_enabled {
                        "DHCP"
                    } else {
                        tr(model.language, "静态", "static")
                    }
                ),
            ),
        ]);
    }
    local.extend([
        line(
            tr(model.language, "下载", "Download"),
            &format_rate(snapshot.download_bps),
        ),
        line(
            tr(model.language, "上传", "Upload"),
            &format_rate(snapshot.upload_bps),
        ),
        line(
            tr(model.language, "总接收", "Received"),
            &format_bytes(snapshot.total_download),
        ),
        line(
            tr(model.language, "总发送", "Sent"),
            &format_bytes(snapshot.total_upload),
        ),
    ]);
    frame.render_widget(
        Paragraph::new(local)
            .block(Block::bordered().title(tr(model.language, " 本地概览 ", " Local Overview ")))
            .wrap(Wrap { trim: true }),
        cols[0],
    );
    let mut public = vec![
        line(
            tr(model.language, "代理", "Proxy"),
            snapshot
                .proxy
                .as_deref()
                .unwrap_or(tr(model.language, "无", "none")),
        ),
        line(
            tr(model.language, "状态", "Status"),
            task_label(&model.dashboard.status, model.language),
        ),
    ];
    if let Some(info) = &snapshot.public_info {
        let location = [&info.city, &info.region, &info.country]
            .into_iter()
            .filter(|part| !part.is_empty())
            .cloned()
            .collect::<Vec<_>>()
            .join(", ");
        public.extend([
            line("Public IP", &info.ip),
            line(tr(model.language, "位置", "Location"), &location),
            line("ISP", &info.isp),
        ]);
    } else {
        public.push(line(
            "Public IP",
            tr(model.language, "正在获取…", "loading…"),
        ));
    }
    if let Some(error) = &model.dashboard.error {
        public.push(line(tr(model.language, "错误", "Error"), &error.message));
    }
    public.extend([
        Line::from(""),
        Line::from(Span::styled(
            tr(
                model.language,
                "在线展览只使用模拟数据，不会访问您的局域网。",
                "The online exhibit uses simulated data and never accesses your LAN.",
            ),
            Style::default().fg(Color::Yellow),
        )),
    ]);
    frame.render_widget(
        Paragraph::new(public)
            .block(Block::bordered().title(tr(model.language, " 公网信息 ", " Public Network ")))
            .wrap(Wrap { trim: true }),
        cols[1],
    );
}

fn render_adapters(frame: &mut Frame, area: Rect, model: &AppModel, ui: &mut UiState) {
    if let Some(edit) = &model.adapters.edit {
        render_adapter_edit(frame, area, model, edit, ui);
        return;
    }
    let areas =
        Layout::vertical([Constraint::Percentage(58), Constraint::Percentage(42)]).split(area);
    let table_inner = Block::bordered().inner(areas[0]);
    for index in 0..model
        .adapters
        .items
        .len()
        .min(table_inner.height.saturating_sub(1) as usize)
    {
        ui.adapter_regions.push((
            Rect::new(
                table_inner.x,
                table_inner.y + 1 + index as u16,
                table_inner.width,
                1,
            ),
            index,
        ));
    }
    let rows = model
        .adapters
        .items
        .iter()
        .enumerate()
        .map(|(index, adapter)| {
            let style = if index == model.adapters.selected {
                Style::default().bg(SELECTED)
            } else {
                Style::default()
            };
            Row::new(vec![
                Cell::from(adapter.name.clone()),
                Cell::from(adapter.kind.clone()),
                Cell::from(adapter.ipv4.clone()),
                Cell::from(adapter.mac.clone()),
                Cell::from(adapter.status.clone()),
            ])
            .style(style)
        });
    frame.render_widget(
        Table::new(
            rows,
            [
                Constraint::Percentage(23),
                Constraint::Length(10),
                Constraint::Length(17),
                Constraint::Length(18),
                Constraint::Min(8),
            ],
        )
        .header(
            Row::new(["Adapter", "Type", "IPv4", "MAC", "Status"])
                .style(Style::default().fg(PRIMARY).add_modifier(Modifier::BOLD)),
        )
        .block(Block::bordered().title(format!(
            " {} · {} ",
            tr(model.language, "适配器", "Adapters"),
            task_label(&model.adapters.status, model.language).trim()
        ))),
        areas[0],
    );

    let details = model
        .adapters
        .items
        .get(model.adapters.selected)
        .map(|adapter| {
            vec![
                line(
                    tr(model.language, "描述", "Description"),
                    &adapter.description,
                ),
                line(
                    "SSID",
                    adapter
                        .ssid
                        .as_deref()
                        .unwrap_or(tr(model.language, "无", "none")),
                ),
                line(
                    tr(model.language, "寻址", "Addressing"),
                    if adapter.dhcp_enabled {
                        "DHCP"
                    } else {
                        "static"
                    },
                ),
                line(
                    tr(model.language, "链路速率", "Link rate"),
                    &adapter
                        .link_speed_bps
                        .map_or_else(|| "—".into(), |bits| format_rate(bits.saturating_div(8))),
                ),
                line(
                    tr(model.language, "实时流量", "Live traffic"),
                    &format!(
                        "↓ {}  ↑ {}",
                        format_rate(adapter.download_bps),
                        format_rate(adapter.upload_bps)
                    ),
                ),
                line(
                    tr(model.language, "累计流量", "Totals"),
                    &format!(
                        "↓ {}  ↑ {}",
                        format_bytes(adapter.total_download),
                        format_bytes(adapter.total_upload)
                    ),
                ),
            ]
        })
        .unwrap_or_else(|| {
            vec![Line::from(tr(
                model.language,
                "未发现网络适配器。",
                "No network adapters detected.",
            ))]
        });
    frame.render_widget(
        Paragraph::new(details)
            .block(Block::bordered().title(tr(model.language, " 详情 ", " Details ")))
            .wrap(Wrap { trim: true }),
        areas[1],
    );
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
            Constraint::Length(4),
            Constraint::Length(3),
            Constraint::Min(5),
        ])
        .split(area);
    let input_style = if model.scanner.editing {
        Style::default().fg(Color::Yellow)
    } else {
        Style::default().fg(Color::White)
    };
    frame.render_widget(
        Paragraph::new(format!(
            "CIDR: {}{}",
            model.scanner.cidr,
            if model.scanner.editing { "█" } else { "" }
        ))
        .style(input_style)
        .block(Block::bordered().title(tr(
            model.language,
            " 扫描范围 · E 编辑 ",
            " Scan Range · E to edit ",
        ))),
        rows[0],
    );
    let ratio = if model.scanner.total == 0 {
        0.0
    } else {
        model.scanner.current as f64 / model.scanner.total as f64
    };
    frame.render_widget(
        Gauge::default()
            .block(Block::bordered().title(task_label(&model.scanner.status, model.language)))
            .gauge_style(Style::default().fg(SECONDARY))
            .ratio(ratio.clamp(0.0, 1.0)),
        rows[1],
    );
    ui.scanner_action = Some(rows[1]);

    let table_rows = model
        .scanner
        .results
        .iter()
        .enumerate()
        .map(|(index, host)| {
            Row::new(vec![
                host.ip.clone(),
                host.mac.clone(),
                host.hostname.clone(),
            ])
            .style(if index == model.scanner.selected {
                Style::default().bg(SELECTED)
            } else {
                Style::default()
            })
        });
    frame.render_widget(
        Table::new(
            table_rows,
            [
                Constraint::Length(17),
                Constraint::Length(19),
                Constraint::Min(10),
            ],
        )
        .header(Row::new(["IP", "MAC", "Hostname"]).style(Style::default().fg(PRIMARY)))
        .block(Block::bordered().title(tr(
            model.language,
            " 发现设备 ",
            " Discovered Hosts ",
        ))),
        rows[2],
    );
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
            Row::new([
                "Adapter",
                "Download",
                "Upload",
                "Session RX/TX",
                "Total RX/TX",
            ])
            .style(Style::default().fg(PRIMARY)),
        )
        .block(Block::bordered().title(format!(
            " {} · {} ",
            tr(model.language, "实时流量", "Live Traffic"),
            task_label(&model.traffic.status, model.language).trim()
        ))),
        area,
    );
}

fn render_diagnostics(frame: &mut Frame, area: Rect, model: &AppModel, ui: &mut UiState) {
    let common = model.diagnostics.active_common();
    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Length(20),
            Constraint::Min(30),
            Constraint::Length(26),
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
        items.push(
            ListItem::new(tool_label(tool)).style(if tool == model.diagnostics.tool {
                Style::default().fg(Color::White).bg(SELECTED)
            } else {
                Style::default().fg(PRIMARY)
            }),
        );
    }
    frame.render_widget(
        List::new(items).block(
            Block::bordered()
                .title(tr(model.language, " 工具 ", " Tools "))
                .border_style(focus_style(DiagnosticFocus::Menu)),
        ),
        cols[0],
    );

    ui.diagnostic_main = Some(cols[1]);
    let main_block = Block::bordered()
        .title(format!(" {} ", tool_label(model.diagnostics.tool)))
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
            DiagnosticTool::Trace => {
                let rows = model
                    .diagnostics
                    .trace
                    .hops
                    .iter()
                    .enumerate()
                    .map(|(index, hop)| {
                        Row::new(vec![
                            hop.ttl.to_string(),
                            hop.address.clone().unwrap_or_else(|| "*".into()),
                            hop.latency_ms
                                .map_or_else(|| "—".into(), |value| format!("{value} ms")),
                            hop.hostname.clone().unwrap_or_default(),
                        ])
                        .style(
                            if index == model.diagnostics.trace.selected {
                                Style::default().bg(SELECTED)
                            } else {
                                Style::default()
                            },
                        )
                    });
                frame.render_widget(
                    Table::new(
                        rows,
                        [
                            Constraint::Length(5),
                            Constraint::Length(17),
                            Constraint::Length(10),
                            Constraint::Min(8),
                        ],
                    )
                    .header(
                        Row::new(["Hop", "Address", "RTT", "Host"])
                            .style(Style::default().fg(PRIMARY)),
                    ),
                    main_inner,
                );
            }
            _ => {
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

    let config_rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(7), Constraint::Length(3)])
        .split(cols[2]);
    let config_block = Block::bordered()
        .title(tr(model.language, " 配置 ", " Configuration "))
        .border_style(focus_style(DiagnosticFocus::Config));
    let config_inner = config_block.inner(config_rows[0]);
    frame.render_widget(config_block, config_rows[0]);
    let fields = diagnostic_fields(model);
    for (index, (label, raw_value)) in fields.into_iter().enumerate() {
        if index >= config_inner.height as usize {
            break;
        }
        let row = Rect::new(
            config_inner.x,
            config_inner.y + index as u16,
            config_inner.width,
            1,
        );
        let value_x = row.x.saturating_add(11.min(row.width));
        ui.diagnostic_fields.push((row, index, value_x));
        let selected = model.diagnostics.focused
            && model.diagnostics.focus == DiagnosticFocus::Config
            && active_diagnostic_config_index(model) == index;
        let text_editable = index == 0 || model.diagnostics.tool == DiagnosticTool::Trace;
        let mut spans = vec![Span::styled(
            format!("{} {label:<8}", if selected { ">" } else { " " }),
            if selected {
                Style::default().fg(Color::Yellow)
            } else {
                Style::default().fg(MUTED)
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
            if index == 0
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
        frame.render_widget(Paragraph::new(Line::from(spans)), row);
    }
    frame.render_widget(
        Gauge::default()
            .block(Block::bordered().title(task_label(&common.status, model.language)))
            .gauge_style(Style::default().fg(SECONDARY))
            .percent(common.progress.min(100) as u16),
        config_rows[1],
    );

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
        _ => vec![(
            tr(model.language, "目标", "Target"),
            model.diagnostics.active_target().to_string(),
        )],
    }
}

fn render_settings(frame: &mut Frame, area: Rect, model: &AppModel) {
    let language = match model.language {
        Language::Zh => "简体中文",
        Language::En => "English",
    };
    let rows = vec![
        Row::new(["Language".to_string(), language.to_string()]),
        Row::new([
            "Scan concurrency".to_string(),
            model.scan_concurrency.to_string(),
        ]),
        Row::new([
            "Runtime".to_string(),
            if model.demo { "Demo" } else { "Native" }.to_string(),
        ]),
        Row::new([
            "Persistence".to_string(),
            "Browser local / native config.json".to_string(),
        ]),
    ];
    frame.render_widget(
        Table::new(rows, [Constraint::Length(24), Constraint::Min(20)])
            .block(Block::bordered().title(tr(model.language, " 设置 ", " Settings "))),
        area,
    );
}

fn render_footer(frame: &mut Frame, area: Rect, model: &AppModel) {
    frame.render_widget(
        Paragraph::new(tr(
            model.language,
            " [Tab] 切页  [方向键] 导航  [Enter/Space] 开始/停止  [E] 编辑  [F1] 帮助 ",
            " [Tab] Pages  [Arrows] Navigate  [Enter/Space] Start/Stop  [E] Edit  [F1] Help ",
        ))
        .style(Style::default().fg(MUTED))
        .alignment(Alignment::Center),
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

fn tool_label(tool: DiagnosticTool) -> &'static str {
    match tool {
        DiagnosticTool::Ping => "Ping",
        DiagnosticTool::Trace => "Trace Route",
        DiagnosticTool::PortScan => "Port Scan",
        DiagnosticTool::PublicSpeed => "Public Speed",
        DiagnosticTool::LinkQuality => "Link Quality",
        DiagnosticTool::LanSpeed => "LAN Speed",
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

fn line(label: &str, value: &str) -> Line<'static> {
    Line::from(vec![
        Span::styled(format!("{label:<12}"), Style::default().fg(MUTED)),
        Span::styled(value.to_string(), Style::default().fg(Color::White)),
    ])
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
                assert_eq!(ui.hit_test(2, 5), Some(Action::SelectAdapter(0)));

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
                        assert!(text.contains("open: 443"));
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
            assert_eq!(
                ui.hit_test(107, 4),
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
}
