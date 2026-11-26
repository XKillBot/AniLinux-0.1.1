# AniLinux

**Aplicación 0.1.1 — corrección de reproducción.** Consulta [CAMBIOS-0.1.1.md](CAMBIOS-0.1.1.md) si vienes de la entrega inicial. Esta corrección requiere recompilar la aplicación; no se distribuye mediante la actualización de la fuente.

Cliente **nativo de escritorio para Linux**, escrito en Rust con **egui / eframe** y renderizado OpenGL. Explora AnimeAV1, abre episodios en un reproductor externo y guarda vídeos para verlos sin conexión. No usa Electron, WebViews, APKs ni cuentas de usuario.

> Este es un proyecto funcional inicial, no una garantía de compatibilidad permanente con todos los alojamientos. Usa solamente contenido que tengas autorización para reproducir o descargar. No implementa evasión de DRM, CAPTCHA, autenticación ni restricciones de acceso.

## Inicio rápido — Debian / Ubuntu

Necesitas Rust **stable actualizado**, una sesión gráfica X11 o Wayland y acceso a Internet para consultar la fuente.

```bash
sudo apt update
sudo apt install -y build-essential pkg-config \
  libx11-dev libxi-dev libxcursor-dev libxrandr-dev libxinerama-dev \
  libwayland-dev libxkbcommon-dev libxkbcommon-x11-0 \
  libgl1-mesa-dev libegl1-mesa-dev \
  mpv vlc yt-dlp ffmpeg xdg-utils ca-certificates
```

Instala Rust desde <https://rustup.rs> si aún no tienes `cargo`, o actualiza tu instalación existente:

```bash
rustup update stable
cd anilinux
cargo build --release --locked
./target/release/anilinux
```

No lo ejecutes con `sudo`. La primera compilación descarga las dependencias de Rust. Las siguientes aprovechan la caché. No necesitas Node.js, un servidor web ni un navegador para la interfaz.

### Fedora

```bash
sudo dnf install gcc gcc-c++ make pkgconf-pkg-config \
  libX11-devel libXi-devel libXcursor-devel libXrandr-devel libXinerama-devel \
  wayland-devel libxkbcommon-devel libxkbcommon-x11 \
  mesa-libGL-devel mesa-libEGL-devel xdg-utils
```

Instala además `mpv`, `yt-dlp` y una distribución de FFmpeg con los códecs necesarios desde los repositorios que tengas habilitados. Su disponibilidad depende de la edición de Fedora y de los repositorios multimedia configurados.

### Arch Linux

```bash
sudo pacman -S --needed base-devel pkgconf libx11 libxi libxcursor \
  libxrandr libxinerama wayland libxkbcommon libxkbcommon-x11 mesa \
  mpv yt-dlp ffmpeg xdg-utils
```

En las tres distribuciones, la compilación es la misma: `cargo build --release --locked`.

### Problemas de inicio gráfico

- Ejecuta la aplicación dentro de tu sesión de escritorio, con `DISPLAY` o `WAYLAND_DISPLAY` disponible.
- Si falta `libxkbcommon-x11.so`, instala el paquete de ejecución indicado arriba.
- Para aislar un problema del controlador gráfico: `LIBGL_ALWAYS_SOFTWARE=1 ./target/release/anilinux` (solo diagnóstico; puede consumir más CPU).
- Se usa TLS de Rust (`rustls`); no se necesita OpenSSL como dependencia de compilación de este proyecto.

## Uso

1. **Inicio** muestra carruseles de episodios recientes y series agregadas, obtenidos en cada consulta. Tras abrir episodios aparece «Continuar viendo».
2. **Catálogo** ofrece tarjetas con portada, búsqueda remota, géneros y paginación. El historial de búsqueda empieza vacío y guarda únicamente lo que escribas; puedes borrarlo.
3. **En emisión** consulta el filtro real de emisión de la fuente.
4. Al abrir una tarjeta se consulta la ficha: portada/banner, descripción, año/tipo si existen, géneros y episodios ordenados.
5. Pulsa un episodio o «Ver primer episodio» para elegir **idioma y servidor**. **DUB solo aparece cuando el episodio tiene enlaces DUB**. No se infiere doblaje del título ni se crea un botón DUB vacío.
6. **Reproducir** resuelve el enlace antes de enviarlo a mpv o VLC. No confunde la página de un embed con un archivo de vídeo.
7. El **interruptor de cada episodio inicia una descarga real**, usando SUB cuando existe o el primer idioma disponible y priorizando el servidor HLS. Actívalo desde la ficha para una descarga rápida; para escoger otro idioma/servidor usa el diálogo del episodio. Apagar un interruptor activo cancela la descarga; un episodio terminado permanece marcado.
8. **Descargas** muestra cola, progreso cuando el servidor lo proporciona, cancelación y reproducción local. Solo se ejecutan tres descargas a la vez. Quitar un elemento de la lista no borra el archivo.
9. **Opciones** permite elegir la carpeta y el reproductor. La carpeta se crea al descargar. Usa una ruta absoluta o `~/…`.

### Reproducción y descargas

- En automático se elige **mpv** si está instalado; si no, **VLC**. No se cambia automáticamente de reproductor tras un fallo del vídeo. Puedes elegir uno explícitamente en Opciones. La ventana del reproductor es externa.
- mpv recibe argumentos `--opción=valor` y se conecta mediante un socket privado. AniLinux espera el evento `playback-restart` antes de mostrar «Reproducción iniciada». El arranque tiene límites de tiempo y detecta errores de apertura. No confunde un proceso vivo con un vídeo reproduciéndose.
- Con `LIBGL_ALWAYS_SOFTWARE=1`, mpv usa OpenGL permitiendo renderizado por software (`--gpu-sw=yes`) y sin decodificación de vídeo por hardware. Se conserva este modo en la aplicación.
- VLC se detecta en instalaciones nativas (`PATH`, `/usr/bin/vlc`), Snap (`/snap/bin/vlc`) y Flatpak (`org.videolan.VLC`), tanto para archivos locales descargados como para streaming HTTP y HLS (`.m3u8`). En modo software (`LIBGL_ALWAYS_SOFTWARE=1`) desactiva la decodificación por hardware y prioriza salidas de vídeo por software (`xcb-x11,xcb-xv,wl,any`). Puedes cambiar entre `Auto`, `mpv` y `VLC` desde **Opciones**, **Descargas** o el diálogo del episodio.
- mpv admite todas las cabeceras personalizadas de un servidor; VLC adjunta `Referer` y `User-Agent` como opciones de entrada (`:http-referrer` / `:http-user-agent`) y acepta cabeceras estándar de navegador sin rechazar la reproducción.
- `xdg-open` se admite para **archivos locales** con la aplicación predeterminada. No se presenta como solución universal para streaming: no permite transportar cabeceras HTTP de manera fiable y suele abrir un navegador con enlaces HTTPS. Instala mpv o VLC para ver episodios en línea.
- «Continuar» recuerda el **último episodio abierto**, no certifica que se haya visto completo ni recoge el minuto de reproducción de reproductores externos.
- Se admiten de forma nativa **formatos abiertos estándar**: listas HLS (`.m3u8` / `.m3u` o respuestas `application/vnd.apple.mpegurl` / `#EXTM3U`) y archivos multimedia directos por HTTP (`.mp4`, `.mkv`, `.webm`, `.ogv`, `.mov`, `.m4v`), además del apartado **Enlace directo (.m3u8 / HTTP)** en **Descargas** para fuentes propias o autorizadas.
- Los archivos directos se descargan con HTTP asíncrono, se comprueba el estado HTTP, se rechazan respuestas HTML y se renombra el archivo provisional únicamente al terminar.
- Para HLS se usa **yt-dlp con FFmpeg** (o **FFmpeg** directamente si `yt-dlp` no está instalado); la salida se remultiplexa a MKV sin recodificar el vídeo y sin sobrescribir archivos terminados existentes.
- Los embeds que no tienen resolvedor declarativo usan la extracción de `yt-dlp` (`best`, formato combinado). La compatibilidad depende del servidor y de la versión instalada de esa herramienta. Un enlace a Mega/TransferIt/otro alojamiento **no se trata como un MP4**: si no puede resolverse, se notifica el fallo y debes elegir otro servidor.
- Mantén la aplicación abierta durante las descargas. Al salir se solicita cancelación; las tareas interrumpidas aparecen como canceladas al volver. **No hay reanudación de descargas entre sesiones** en esta versión.

## Arquitectura

```text
anilinux/
├── Cargo.toml / Cargo.lock
├── src/
│   ├── main.rs                # ventana, runtime y cierre
│   ├── lib.rs                 # módulos reutilizables y pruebas
│   ├── ui.rs                  # navegación, tarjetas, ficha, diálogo y descargas
│   ├── tasks.rs               # coordinación asíncrona, eventos y cola
│   ├── models.rs              # modelos independientes de egui
│   ├── parser.rs              # SvelteKit + fallback HTML; sin ejecutar JS
│   ├── extension_manager.rs   # versiones, validación, respaldo y activación
│   ├── resolver.rs            # embed -> vídeo; reglas y fallback yt-dlp
│   ├── player.rs              # procesos mpv / VLC / xdg-open
│   ├── downloader.rs          # HTTP / HLS, progreso y cancelación
│   ├── images.rs              # red, decodificación, caché y texturas
│   ├── network.rs             # cliente HTTP, límites y cabeceras
│   └── storage.rs             # estado local y escrituras atómicas
├── extension/animeav1.json    # definición incluida al compilar
├── animeav1_extension.json    # definición lista para publicar
├── extension_manifest.json   # manifiesto listo para publicar
├── examples/probe.rs          # prueba de integración de red, sin GUI
├── tests/                    # pruebas independientes de la web
├── tools/prepare_extension.py
└── packaging/                # entrada .desktop e icono
```

La UI recibe eventos y dibuja; no hace peticiones, descarga ni espera procesos. Tokio tiene cuatro hilos de trabajo; el parseo HTML y la decodificación de imágenes se ejecutan con `spawn_blocking`. Las portadas se transfieren con un máximo de seis peticiones simultáneas, con texturas reducidas y una caché RAM de hasta 96 imágenes. La caché en disco se recorta a 256 MiB al iniciar. Las respuestas tardías de una búsqueda/ficha anterior se descartan mediante identificadores de petición.

### Parser SvelteKit

No intenta convertir todo `kit.start(...)` a JSON ni usa `eval`. Tokeniza literales JavaScript en los scripts con el marcador configurado, respeta comillas/escapes, arrays, objetos anidados, claves sin comillas y saltos de línea. Tolera referencias compartidas (`a`, `b`) y `void 0` como valores desconocidos; extrae los campos conocidos sin depender del orden de SUB y DUB. No ejecuta funciones o scripts remotos. Tiene límites de respuesta, imagen y profundidad.

El catálogo usa `results`, `latestMedia` y `latestEpisodes`; los géneros usan sus **slugs**, no sus IDs numéricos. Hay fallback de tarjetas HTML. Las expresiones de `patterns` de la extensión remota v1 siguen aceptándose como fallback para episodios/enlaces. No hay `.unwrap()` sobre datos remotos.

No se incluyen animes de demostración, títulos de las capturas, historiales precargados ni portadas empaquetadas. Las pruebas usan datos sintéticos, no una sesión del sitio.

## Actualizaciones de la fuente

La dirección está incorporada **solo en el código**, nunca se muestra ni se solicita en la interfaz:

```text
https://raw.githubusercontent.com/XKillBot/AniLinux/main/extension_manifest.json
```

Flujo de inicio:

1. Cargar y validar `extension.json` local; si falla, probar `extension.previous.json`; si también falla, usar la definición incluida.
2. Publicar esa definición a la UI **antes** de esperar el manifiesto remoto. El catálogo puede cargar mientras se comprueba la actualización.
3. Consultar el manifiesto en segundo plano y comparar `version_code` si ambos lo tienen; en caso contrario, comparar `version` con SemVer.
4. Si hay una versión más nueva, mostrar un aviso «Hay una actualización de la fuente disponible» y un botón **Actualizar**.
5. Descargar el JSON, comprobar SHA-256 cuando `sha256.trim()` no está vacío, validar definición, selectores, reglas y coincidencia de versión/ID con el manifiesto.
6. Respaldar la última definición válida, escribir la nueva con un archivo temporal + sincronización + renombrado y recargar las vistas sin reinstalar AniLinux.

Un hash vacío o con solo espacios se omite sin error, como en tu manifiesto actual. Un hash informado e incorrecto impide la instalación. Si falla la conexión, la descarga o la validación, se conserva la versión anterior.

**Importante:** es una extensión **declarativa**, no un complemento Rust ni un ejecutable descargado. Actualiza rutas, selectores, nombres de campos raíz, plantillas de imágenes, regex de fallback y reglas de resolución URL. No puede incorporar por sí sola cualquier algoritmo futuro, una API cifrada nueva o lógica arbitraria de un alojamiento: esos cambios pueden requerir actualizar el motor Rust o yt-dlp. Esto evita ejecutar código remoto en el equipo.

### Publicar una revisión — solo para mantenimiento

La URL suministrada por el usuario ya existe y usa el formato compatible `id`, `version`, `version_code`, `extension_url`, `sha256`. El proyecto no modifica ni publica archivos de tu repositorio automáticamente.

1. Edita `extension/animeav1.json`: modifica las reglas y aumenta `version_code` y `version`.
2. Ejecuta:

   ```bash
   python3 tools/prepare_extension.py
   cargo test --locked
   cargo run --locked --example probe
   ```

3. Publica juntos `animeav1_extension.json` y `extension_manifest.json` en la raíz de la rama `main` de tu repositorio. El script calcula el hash de los **bytes exactos** de la definición a publicar.

Si modificas el JSON publicado después de calcular el hash, debes regenerar el manifiesto. Es recomendable usar el hash, aunque se acepta vacío por compatibilidad. SHA-256 comprueba integridad respecto del manifiesto; **no sustituye una firma digital** si el propio repositorio o manifiesto se compromete. Esta versión confía en HTTPS y en el repositorio configurado.

La revisión empaquetada comienza en `1.0.0` / código `1`, igual que el manifiesto consultado. Por eso no aparece un aviso de actualización en una instalación limpia hasta que publiques una revisión mayor.

## Datos locales

Se respetan las rutas XDG mediante `dirs`:

| Dato | Ubicación predeterminada |
|---|---|
| Estado, opciones, historial y descargas registradas | `~/.local/share/anilinux/library.json` |
| Definición activa y respaldo | `~/.local/share/anilinux/extension.json`, `extension.previous.json` |
| Imágenes | `~/.cache/anilinux/covers/` |
| Vídeos | `~/Videos/AniLinux/` o el directorio de vídeo XDG, configurable |

El historial es local; AniLinux no implementa sincronización, cuentas ni funciones sociales. Las rutas internas, la URL de actualización, selectores, patrones y código no se presentan en menús, avisos ni ventanas de información. La única ruta editable en la UI es la carpeta de descarga que solicita el flujo de uso.

## Instalar en el menú del escritorio

Después de compilar:

```bash
mkdir -p ~/.local/bin ~/.local/share/applications ~/.local/share/icons/hicolor/scalable/apps
install -m755 target/release/anilinux ~/.local/bin/anilinux
install -m644 packaging/anilinux.desktop ~/.local/share/applications/anilinux.desktop
install -m644 packaging/anilinux.svg ~/.local/share/icons/hicolor/scalable/apps/anilinux.svg
```

Asegúrate de que `~/.local/bin` está en el `PATH` de la sesión gráfica; si no, utiliza una ruta absoluta en `Exec=` de la entrada `.desktop`.

## Pruebas y alcance de validación

```bash
cargo fmt --all -- --check
cargo clippy --all-targets --locked -- -D warnings
cargo test --all-targets --locked
cargo run --locked --example probe
```

- Las **18 pruebas automatizadas** no dependen de Internet. Cubren literales/escapes, episodios fraccionarios y ordenados, paginación, búsqueda codificada, idiomas reales, DUB vacío, fallback HTML, entrada inválida, versiones, SHA-256 vacío/válido/incorrecto, activación atómica con respaldo y conservación tras fallos, descarga HTTP contra servidor de prueba, rechazo de HTML y nombres seguros.
- Hay además **3 pruebas de integración optativas** con mpv real: reproducción local, streaming HTTP y rechazo de un HTTP 404 sin anunciar éxito. En esta revisión se ejecutaron las tres con mpv 0.40.0, X11 virtual, vídeo sintético sin audio y OpenGL por software. No implican una prueba en el equipo del usuario ni de cada alojamiento externo. Las instrucciones están en `CAMBIOS-0.1.1.md`.
- `probe` sí necesita red: consulta catálogo, abre una ficha obtenida dinámicamente, lee los servidores de su primer episodio y resuelve el preferido. No lanza un reproductor ni descarga un episodio entero. Opcionalmente acepta una URL de ficha como argumento para desarrollo.
- En la preparación del proyecto se compilaron los objetivos, se ejecutaron las pruebas y se arrancó la GUI real en X11 virtual con OpenGL por software. También se consultó AnimeAV1 realmente y se validó la resolución del manifiesto HLS.
- **No se ha verificado reproducción audiovisual completa ni una descarga completa de cada alojamiento externo.** La prueba HTTP local no implica que todos los enlaces de terceros funcionen. Tampoco se ha probado físicamente cada distribución, controlador gráfico o compositor Wayland.
- Si AnimeAV1 responde con un bloqueo o cambia su estructura, la aplicación muestra el error y conserva la fuente local; no intenta eludir protecciones.

Licencia MIT para el código de AniLinux. Los contenidos y marcas de terceros no están incluidos ni cubiertos por esta licencia.
