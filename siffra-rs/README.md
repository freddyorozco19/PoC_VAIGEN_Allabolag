# Siffra — réplica en Rust

Réplica del scaffold Next.js (`../src`) en Rust: servidor **Axum** con HTML renderizado en servidor con **maud**.
Mismas páginas, textos, datos de EJEMPLO, gráficos SVG y tokens de diseño (tema claro/oscuro según el sistema).
Sigue siendo un scaffold: los datos de las empresas son ficticios (ver `../README.md`). Lo único real son las
**medianas del sector**, que se piden a la base de estadísticas abierta de SCB (`src/scb.rs`, sin clave): sale una
llamada de red por rama/tamaño y se cachea 24 h. Con `SCB_STATS_DISABLED=1` se usan las medianas de EJEMPLO.

**Bolagsverket (datos reales de empresas):** `src/bolagsverket.rs` consulta el API gratuito por organisationsnummer
(OAuth2 + `/organisationer`). Lee las claves de `.env.local` (en `siffra-rs/` o en la raíz del repo; git lo ignora) —
`BOLAGSVERKET_CLIENT_ID`, `BOLAGSVERKET_CLIENT_SECRET`, `BOLAGSVERKET_BASE_URL` (test: `gw-accept2…`, producción: `gw…`;
el token se deduce de la URL base). Con claves, `/foretag/<organisationsnummer>` y el buscador muestran la empresa real
(nombre, dirección, SNI, estado, konkurs/likvidation); sin claves, solo los datos de EJEMPLO. El API no busca por
nombre y no incluye cifras financieras: esas siguen siendo de EJEMPLO. Se valida el dígito de control (Luhn) antes
de llamar, se cachea 10 min y se rechazan los identificadores de 12 dígitos (personnummer).
Prueba contra el entorno real: `cargo test live_bolagsverket_lookup -- --ignored --nocapture`.

## Ejecutar

```bash
cd siffra-rs
cargo run          # http://localhost:3001  (PORT=3000 cargo run para cambiar el puerto)
cargo test         # unidades + pruebas de las rutas (sin red)
cargo test -- --ignored   # prueba contra el API real de SCB (requiere red)
```

Requiere Rust estable (probado con 1.99). Las tipografías se cargan desde Google Fonts.

## Interfaz (refinada respecto al original)

Mismo mundo visual (paleta, tipografías, contenido), con la experiencia mejorada:

- **Accesibilidad:** contrastes WCAG AA en claro y oscuro (los tokens `*-ink`, `--bar2` y `--line-strong` se ajustaron
  para ello), foco visible, enlace "Hoppa till innehållet", un `h1` por página, tablas con `caption` y `scope`,
  pestañas como navegación (`aria-current`), severidad con icono + texto (no solo color), gráficos con título,
  descripción y tabla de valores.
- **Móvil:** barra superior + pestañas inferiores con iconos; tablas que priorizan columnas.
- **Tema:** claro / oscuro / sistema (botón en la cabecera, se recuerda en `localStorage`).
- **Interacción:** tooltips con ratón, teclado (Tab) y toque; barras de los gráficos enfocables con hover; términos
  financieros explicados; copiar el organisationsnummer; búsqueda ordenable; atajo `/` para ir al buscador.
- **Rendimiento percibido:** si SCB aún no está en caché, la ficha sale al instante con un esqueleto y pide
  `/foretag/<org>/benchmarks` en segundo plano.

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
