# v0.3.1 UI compatibility contract

The v0.4 architecture may replace state ownership and runtime boundaries, but it must not redesign
the terminal experience as a side effect. The native v0.3.1 renderer at commit `9418468` is the
visual and interaction reference. The retained native renderer remains the executable reference
until the shared native entry point replaces it.

## Scope

- The terminal surface is shared by native demo and Web. The Web page shell may add preview,
  scenario, language, fullscreen and download controls outside the terminal.
- Exact pixel equality is not required across terminal fonts and browser renderers. Panel hierarchy,
  proportions, information, color roles, focus, selection and shortcut placement are required.
- A shared renderer means shared backend-independent Ratatui code. It does not mean that unrelated
  tools are reduced to one generic log panel.

## Shell invariants

| Element | v0.3.1 contract |
|---|---|
| Header | Three terminal rows, bordered title `IP Tools CLI`; demo adds a suffix only. |
| Tabs | Six localized labels, `|` separators, green inactive text, dark-gray selected background. |
| Body | Uses all remaining rows; no Web-only padding inside the terminal. |
| Footer | One left-aligned row with switch, language, help and quit shortcuts. |
| Focus | Yellow diagnostic panel border; selected list row uses dark gray; status uses green/red/cyan. |
| Palette | Primary green, secondary cyan, muted gray, subtle dark gray, error red. |

## Page contracts

| Page | Required structure and information |
|---|---|
| Dashboard | 50/50 local and public panels; host/OS, active adapter description, addressing, IP, live and total traffic, proxy, public IP, location and ISP. |
| Adapters | 30/70 adapter list and details; physical/virtual badge, state, SSID, type, DHCP/static, MAC, IPv4/CIDR, IPv6, live and total traffic, edit hint. |
| Scanner | Three-row control, four-column IP/MAC/vendor/hostname results, one-row progress at the bottom. |
| Traffic | One full-width table with interface, RX rate, TX rate, session RX/TX and total RX/TX. |
| Diagnostics | 20/50/30 tool menu, tool-specific main panel and two-line configuration fields. |
| Settings | Settings list plus a three-row edit hint. Reset-session behavior must not disappear during migration. |

## Diagnostic contracts

- Ping: six statistics, latency sparkline, recent log and status.
- Trace: hop/address/RTT/host table and status.
- Port Scan: open/scanned statistics, open-port service table, progress and status.
- Link Quality: adapter header, overall gauge, wired/Wi-Fi dimensions, metrics, latency history,
  Wi-Fi RSSI history and status.
- Public Speed: current/average/peak, downloaded/elapsed, rate history and status.
- LAN Speed: endpoint, TX/RX throughput, history, protocol summary and status.
- Configuration values use the v0.3.1 label line followed by `>> value`; target history and editing
  cursor remain visible and job-scoped fields remain locked while running.

## Verification gates

1. `iptools-ui` renders 80x24, 120x36 and 160x48 in Chinese and English without panic or overflow.
2. Tests assert information that was previously lost (for example scanner vendor, ping log/status and
   adapter IPv6), not only page titles.
3. Chromium, Firefox and WebKit exercise the same reducer through DOM and Canvas modes.
4. The DOM grid height and Ratatui-reported height derive from the same terminal container; shell,
   logs, status and footer must all remain visible.
5. Before native cutover, manual comparison uses the retained v0.3.1 native renderer on the left and
   `iptools --demo`/Web on the right at the same terminal dimensions and language.
