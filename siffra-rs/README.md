# Siffra — réplica en Rust

Réplica del scaffold Next.js (`../src`) en Rust: servidor **Axum** con HTML renderizado en servidor con **maud**.
Mismas páginas, textos, datos de EJEMPLO, gráficos SVG y tokens de diseño (tema claro/oscuro según el sistema).
Sigue siendo un scaffold: los datos son ficticios y no hay llamadas de red (ver `../README.md`).

## Ejecutar

```bash
cd siffra-rs
cargo run          # http://localhost:3001  (PORT=3000 cargo run para cambiar el puerto)
cargo test         # unidades + pruebas de las rutas
```

Requiere Rust estable (probado con 1.99). Las tipografías se cargan desde Google Fonts.

## Equivalencias

| Next.js | Rust |
|---|---|
| `src/lib/types.ts`, `src/lib/mock/companies.ts` | `src/model.rs` |
| `src/lib/format.ts` (`toLocaleString("sv-SE")`) | `src/format.rs` |
| `src/lib/ai-summary.ts` | `src/summary.rs` |
| `src/components/*`, `src/app/**/page.tsx` | `src/views.rs` |
| `app/layout.tsx`, rutas, `redirect("/sok")`, `notFound()` | `src/main.rs` |
| Tailwind + `globals.css` | `static/styles.css` (CSS a mano) |

## Diferencias deliberadas

- **Búsqueda** (`/sok?q=…`): en Next filtra en el cliente; aquí es un `GET` y un script mínimo reenvía el formulario al escribir.
- **Pestañas de la ficha** (`/foretag/<org>?tab=ov|fin|ppl|ai`): enlaces en lugar de estado de React.
- **Puerto** 3001 por defecto, para poder correr junto a `npm run dev` (3000).
- Las tablas quitan el borde inferior de la última *fila*; el original lo quitaba de la última *celda* de cada fila (`last:border-0` sobre `td`), lo que parece un descuido.
- Los stubs de `src/lib/data-sources/` (Bolagsverket, SCB) no se han portado: no tienen lógica, solo lanzan "no implementado".
