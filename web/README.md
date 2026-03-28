# PeerClaw web (Vite + React + TypeScript + shadcn/ui)

## Daily development (best experience)

1. Start the Rust node (API + WebSocket + optional legacy embeds):

   ```bash
   peerclaw serve --web 127.0.0.1:8080
   ```

2. In another terminal, from this directory:

   ```bash
   npm install
   npm run dev
   ```

3. Open **http://127.0.0.1:5173** — Vite proxies `/api`, `/v1`, `/ws`, and `/embed/*` to `:8080`, so you get hot reload on the React app while talking to the real node.

## Production static files

```bash
npm run build
```

If `web/dist/index.html` exists, `peerclaw serve` serves the SPA and keeps legacy HTML at `/embed/chat` and `/embed/console`. You can also point to any built folder:

```bash
export PEERCLAW_WEB_DIST=/absolute/path/to/dist
peerclaw serve --web 0.0.0.0:8080
```

## shadcn components

This repo includes the shadcn config (`components.json`) and a few primitives (`button`, `card`). Add more:

```bash
cd web
npx shadcn@latest add dialog table tabs
```

## Tech stack

- [Vite](https://vite.dev/) — dev server, HMR, proxy
- [React Router](https://reactrouter.com/) — `/` assistant + workspace shell (`?view=home|overview|crews|…`), `/console/*` redirects for old bookmarks (`/console/join` → P2P Network `#join-mesh`)
- [Tailwind CSS](https://tailwindcss.com/) + [shadcn/ui](https://ui.shadcn.com/)
- [d3](https://d3js.org/) — force graphs (network + swarm), same idea as the legacy HTML

## Operator UI highlights

- **Home** — Onboarding checklist, chat scenario starters, links to P2P (Join the mesh), and **Crew builder**.
- **P2P Network** — Resources, **Join the mesh** (stats + `serve` commands), libp2p dial/peers graph, swarm tabs.
- **Crews** — Sidebar panel to edit `CrewSpec` (agents, tasks, process), validate, kick off runs, and inspect recent run records.
- **Chat** — Agentic loop, tasks, MCP toggle; flows remain API/SDK-first (`/api/flows/*`).

The React app replaces the embedded `chat.html` / `dashboard.html` when `web/dist` is present. Legacy HTML remains at `/embed/chat` and `/embed/console` on the Rust server if you need a fallback.
