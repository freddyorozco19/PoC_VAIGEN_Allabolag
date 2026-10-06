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
nombre. Se valida el dígito de control (Luhn) antes de llamar, se cachea 10 min y se rechazan los identificadores
de 12 dígitos (personnummer).

**Cifras financieras reales (cuentas anuales):** `src/annual_report.rs` lista los documentos (`/dokumentlista`),
descarga hasta 3 informes (`/dokument/{id}`, un ZIP con iXBRL), los lee y los une para mostrar hasta 5 ejercicios de
omsättning, resultado, eget kapital y tillgångar (en tkr), con soliditet y margen calculados. La ficha sale al
instante y las cifras llegan en segundo plano (`/foretag/<org>/bokslut`). Detalles aprendidos de informes reales:
cifras en coronas y también en miles redondeadas (se usa la más precisa), signo en `sign="-"`, ejercicios que no
son año natural, empresas sin facturación (se muestra "—", no 0). Solo hay cuentas para empresas que presentan en
digital (K2/K3/ESEF; sobre todo pymes): para las demás (p.ej. muchas grandes) la ficha lo dice. Las 3 empresas de
EJEMPLO siguen con cifras ficticias.
Prueba contra informes reales ya descargados: `SIFFRA_SAMPLE_REPORTS=<carpeta con x_*> cargo test real_reports_parse -- --ignored`.
Prueba contra el entorno real: `cargo test live_bolagsverket_lookup -- --ignored --nocapture`.

## Ejecutar

```bash
cd siffra-rs
cargo run          # http://localhost:3001  (PORT=3000 cargo run para cambiar el puerto)
cargo test         # unidades + pruebas de las rutas (sin red)
cargo test -- --ignored   # prueba contra el API real de SCB (requiere red)
```

Requiere Rust estable (probado con 1.99). Las tipografías se cargan desde Google Fonts. Para entrar en local, crea una cuenta: `SIFFRA_DB=siffra.db cargo run -- user-add --role superadmin --name "Yo" --username admin`.

## Valoración financiera y modo demo

Las empresas reales (cualquier número de organización o nombre del registro) tienen en su ficha una **valoración
financiera** calculada con datos reales (`src/analysis.rs`): cuentas anuales digitales de Bolagsverket, mediana del
sector de SCB y estado en el registro. Son reglas simples y explicables, no un modelo; el nivel es la peor de las señales:

| Nivel | Señales |
|---|---|
| Riesgo alto | procedimiento en curso (konkurs, likvidation…), patrimonio neto negativo, pérdidas dos ejercicios seguidos, solidez < 10 % |
| En observación | pérdida en el último ejercicio, caída de facturación ≥ 10 %, último informe con más de 18 meses, baja del registro, solidez o margen por debajo del sector **y además débiles** (solidez < 20 %, margen < 2 %) |
| Riesgo bajo | ninguna de las anteriores (más señales favorables: facturación creciente, beneficios 3+ años seguidos, por encima del sector) |
| Sin valorar | sin cuentas digitales con cifras (muchas grandes cotizadas no las presentan en ese formato) |

Una cifra por debajo de la mediana del sector **no** alerta por sí sola (la mediana de solidez la arrastran las sociedades
holding: una pyme con 28 % frente a 68 % no está en apuros); el gráfico enseña la comparación sin veredicto. No se
valora la liquidez (las cuentas leídas no traen activo ni pasivo corriente) ni la plantilla (la API no la da). El
resumen en lenguaje natural lo escriben plantillas del catálogo con las cifras reales; **no usa ningún modelo de IA**.
Todo es orientativo y no asesoramiento financiero.

**Modo demo** (`SIFFRA_DEMO=1`, apagado por defecto y en producción): activa las 3 empresas ficticias y las pantallas
de maqueta (liquidez, SIE, facturas). Sin él esas rutas dan 404, el menú lleva solo pantallas reales y la portada
invita a buscar (Volvo, Spotify, Ericsson, 556703-7485).

**Medianas del sector (SCB):** consulta la tabla `TAB1270`. SCB retiró su versión en inglés (`lang=en` responde
"Non-existent table"); la aplicación lo detecta y pide en sueco. El código SNI de Bolagsverket llega sin punto
(`71121`) y se convierte a `71.121`, `71.12`, `71.1`, `71` hasta encontrar datos. Como no se conoce la plantilla, se usa la
mediana de todos los tamaños.

## Mis empresas, Comparar e Historial

Tres pantallas con datos reales que completan el menú (en modo normal; las maquetas de liquidez, SIE y facturas siguen
solo en `SIFFRA_DEMO=1` porque necesitarían la contabilidad propia de cada usuario):

- **Mis empresas** (`/bevakning`): se sigue una empresa con el botón de su ficha (máx. 50 por persona). La lista guarda
  la última "foto" de cada una (riesgo, estado del registro, facturación, resultado y solidez del último ejercicio) y
  **marca los cambios** entre revisiones: del nivel de riesgo (p. ej. "Riesgo bajo → Riesgo alto") y del estado en el
  registro (concurso, baja). El cambio se resalta 30 días. La foto se renueva cuando se abre la ficha (cualquier persona
  que la abra actualiza la de todos los que la siguen) o con "Actualizar" / "Actualizar todas" (hasta 8 por pulsación,
  las más antiguas primero; cada revisión consulta Bolagsverket y SCB). La primera foto no cuenta como cambio. No hay
  avisos por correo: los cambios se ven en la pantalla.
- **Comparar** (`/comparar?o=…&o=…`): de 2 a 4 números de organización lado a lado (forma, estado, riesgo, ejercicio,
  facturación, resultado, patrimonio, solidez, margen, crecimiento y mediana del sector), con el mejor valor de cada
  fila resaltado. Desde Mis empresas se marcan casillas y "Comparar seleccionadas".
- **Historial** (`/historial`): las empresas consultadas y las búsquedas recientes de cada persona (salen de la
  actividad ya registrada; el nombre se toma de Mis empresas o del índice del registro).

Los datos viven en la tabla `watchlist` de la base SQLite de cuentas (se crea sola al arrancar).

## Cuentas, roles e idiomas

La aplicación exige iniciar sesión (nginx ya no pide clave). Las cuentas, las sesiones y el registro de actividad viven en
SQLite (`SIFFRA_DB`, por defecto `siffra.db`).

| Rol | Puede |
|---|---|
| `superadmin` | Todo: usuarios de cualquier rol (CRUD), **registro de actividad** (`/activity`, filtros, resumen por usuario, CSV). |
| `admin` | CRUD solo de usuarios con rol `user`. No ve la actividad. |
| `user` | Usar la aplicación y editar su propio perfil e idioma. |

Salvaguardas: nadie se borra, se degrada ni se desactiva a sí mismo; nunca se elimina ni se degrada al último superadmin
activo; un admin no puede asignar otros roles. Contraseñas con argon2id (parámetros OWASP), mínimo 10 caracteres,
contraseña temporal que se muestra una sola vez y obliga a cambiarla, sesiones de 12 h (cookie `HttpOnly`, `SameSite=Lax`,
`Secure` tras HTTPS; el token se guarda como hash), CSRF en todos los formularios, 5 intentos fallidos por IP o por
usuario → bloqueo de 15 min, redirecciones de `?next=` solo internas.

**Seguimiento** (`/activity`, solo superadmin): accesos correctos y fallidos, páginas vistas, búsquedas, empresas
consultadas, cambios de cuentas y de idioma, exportaciones y accesos denegados, con hora (UTC), IP y navegador. Nunca se
guardan contraseñas; los registros se conservan 365 días.

**Idiomas:** español, inglés y sueco (`src/catalog.rs`, una fila `(clave, es, en, sv)` por texto). Se elige con
`?lang=`, la cookie `lang`, el perfil o `Accept-Language` (por defecto, español). Los números siguen el formato de cada
idioma (`64.100 mil SEK` / `64,100 kSEK` / `64 100 tkr`). Los tests comprueban que cada clave usada en el código existe,
que las tres lenguas tienen texto con los mismos marcadores y que ninguna pantalla muestra una clave sin traducir. Los
textos que vienen de Bolagsverket (actividad, forma jurídica) son del registro y se muestran en sueco.

**Línea de comandos** (misma base de datos que el servidor):

```bash
siffra-rs user-add --role superadmin --name "Nombre" --username admin [--password-env VAR] [--must-change]
siffra-rs user-add --role admin --name "Nombre" --email persona@dominio.co [--lang es|en|sv]
siffra-rs user-reset LOGIN [--password-env VAR]   # sin --password-env genera una temporal
siffra-rs user-list
siffra-rs backup /ruta/copia.db                   # copia consistente (VACUUM INTO); el destino no debe existir
```

La primera cuenta se crea con `user-add` (sin cuentas el servidor avisa al arrancar).

## Búsqueda por nombre (índice del registro)

La API gratuita de Bolagsverket solo consulta por número de organización y, según su FAQ, buscar por nombre "no tiene
fecha prevista". Para poder buscar por nombre, `src/registry.rs` usa el archivo oficial **`bolagsverket_bulkfil.zip`**
de las *värdefulla datamängder* (gratis, sin contrato, se actualiza cada semana; ≈250 MB comprimido, ≈1 GB de texto,
≈3 millones de filas). Se lee en streaming y se guarda en un SQLite aparte (`SIFFRA_REGISTRY`, por defecto
`registry.db`, ≈600 MB) con un índice de texto FTS5: cada palabra es un prefijo, se exigen todas, no distingue
mayúsculas ni tildes y busca en todos los nombres de la empresa (incluidos otros idiomas). Una consulta tarda 2–30 ms.

- **No se filtra ninguna fila**: entran también las dadas de baja (casi dos tercios) y las identidades de 12 dígitos
  (personnummer de autónomos, ≈1 millón). Las bajas salen atenuadas y detrás de las activas; las identidades que no son
  número de organización se muestran sin enlace (no tienen ficha en vivo). **Decidir qué filtrar queda pendiente.**
- No se guardan la descripción de la actividad ni la calle (la ficha en vivo las trae).
- `/sok?q=texto` muestra hasta 25 resultados del registro además de las empresas de EJEMPLO; un número de organización
  sigue resolviéndose con la API en vivo.
- **Actualización:** con `SIFFRA_REGISTRY_REFRESH=1` el servidor comprueba cada 6 horas la edad del índice y, si falta o
  tiene más de 7 días, descarga el archivo, construye un índice nuevo (≈30 s, caché de 32 MiB) y lo pone en servicio sin
  parar; si algo falla se conserva el anterior. El zip se borra tras importar.
- **A mano:** `siffra-rs registry-import bolagsverket_bulkfil.zip --out registry.db`, `registry-stats`, `registry-search "volvo"`.
- **Licencia (sin confirmar):** las páginas de Bolagsverket que se pudieron leer no indican licencia para este archivo;
  fuentes de terceros dicen CC BY 4.0 y otras CC0. La búsqueda cita la fuente en pantalla ("Bolagsverket"). Conviene
  confirmarlo con Bolagsverket antes de abrir la aplicación a más gente.

## Interfaz: vidrio líquido

Superficies translúcidas con desenfoque (`backdrop-filter`), borde de luz con brillo especular, sombras en capas con una
escala única de elevación y un fondo vivo de manchas de color que se desplazan despacio. Las superficies con texto son lo
bastante opacas para cumplir WCAG AA sobre cualquier punto del fondo. Alternativas: sin `backdrop-filter`,
`prefers-reduced-transparency` y `prefers-contrast: more` → superficies opacas; `prefers-reduced-motion` → fondo y
animaciones quietos; `forced-colors` → bordes del sistema. La hoja de estilos lleva su huella en la URL
(`/static/styles.css?v=…`) para que un cambio se vea de inmediato.

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
| `app/layout.tsx`, rutas, `redirect("/sok")`, `notFound()` | `src/main.rs` (rutas y CLI), `src/handlers.rs` |
| (nuevo) cuentas, sesiones, idiomas, actividad | `src/app.rs`, `src/auth.rs`, `src/db.rs`, `src/i18n.rs`, `src/catalog.rs`, `src/views_admin.rs` |
| Tailwind + `globals.css` | `static/styles.css` (CSS a mano) |

## Diferencias deliberadas

- **Búsqueda** (`/sok?q=…`): en Next filtra en el cliente; aquí es un `GET` y un script mínimo reenvía el formulario al escribir.
- **Pestañas de la ficha** (`/foretag/<org>?tab=ov|fin|ppl|ai`): enlaces en lugar de estado de React.
- **Puerto** 3001 por defecto, para poder correr junto a `npm run dev` (3000).
- Las tablas quitan el borde inferior de la última *fila*; el original lo quitaba de la última *celda* de cada fila (`last:border-0` sobre `td`), lo que parece un descuido.
- Los stubs de `src/lib/data-sources/` (Bolagsverket, SCB) no se han portado: no tienen lógica, solo lanzan "no implementado".
