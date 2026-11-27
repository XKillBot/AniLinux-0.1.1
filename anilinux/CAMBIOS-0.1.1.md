# AniLinux 0.1.1 — corrección de reproducción

## Fallo reproducido

La entrega 0.1.0 pasaba a mpv `--referrer URL` y `--user-agent VALOR`. mpv exige `--referrer=URL` y `--user-agent=VALOR`, y con la invocación anterior termina con un error de argumentos. La aplicación descartaba stdout/stderr y solo comprobaba si el proceso seguía vivo tras 600 ms; eso ocultaba el diagnóstico y no era una prueba de reproducción.

Este fallo de mpv se reprodujo en el entorno de pruebas. No demuestra por sí solo la causa exacta del fallo de VLC en otro equipo.

## Correcciones

- Argumentos de mpv con `=`; URL enviada después de conectar al socket JSON IPC privado.
- Ventana de mpv solicitada inmediatamente y confirmación por `playback-restart`, no por un temporizador de 600 ms. El evento de apertura de archivo o la aceptación de una orden tampoco se consideran reproducción.
- Detección de errores de apertura y de tiempo agotado. Mensajes de resolución, arranque y apertura separados, sin detalles internos en la interfaz.
- Socket temporal en directorio privado de permisos `0700`, sin puertos TCP de control.
- Cuando se usa `LIBGL_ALWAYS_SOFTWARE=1` o `true`, se pasan a mpv `--hwdec=no --vo=gpu --gpu-api=opengl --gpu-sw=yes`. Esto permite el renderizado OpenGL por software que mpv puede rechazar por defecto. No corrige el controlador gráfico del sistema ni garantiza todos los entornos.
- VLC inicia una instancia nueva sin encolar el vídeo en otra instancia. Con el modo software se desactiva su decodificación por hardware. Su aviso es de proceso iniciado, **no de reproducción confirmada**.
- Diagnósticos de mpv/VLC heredados por el terminal en lugar de descartados. Los fallos del resolvedor también se indican en el terminal.
- No se modifica el extractor, las reglas de servidores, el gestor de extensiones ni el descargador. La reproducción de los alojamientos sigue dependiendo de la resolución de enlaces y de sus condiciones de acceso.

## Instalar esta revisión

Descomprime el paquete en una carpeta nueva para no confundir el ejecutable antiguo con el nuevo:

```bash
unzip AniLinux-0.1.1-source.zip -d AniLinux-corregido
cd AniLinux-corregido/anilinux
cargo build --release --locked
LIBGL_ALWAYS_SOFTWARE=1 ./target/release/anilinux
```

En **Opciones → Reproductor**, selecciona **mpv** explícitamente para la primera prueba. Puedes empezar con **MP4Upload**, que ya permite descargar en tu instalación.

No borres tu carpeta de vídeos ni tus datos locales: esta revisión conserva el formato del estado. La versión de la extensión permanece en 1.0.0 / código 1; la versión de la aplicación pasa a 0.1.1. **Actualizar la fuente en la interfaz no aplica este arreglo: hay que compilar el código nuevo.** Si instalaste el ejecutable en `~/.local/bin`, reemplázalo también:

```bash
install -m755 target/release/anilinux ~/.local/bin/anilinux
```

## Si todavía falla en tu equipo

Comprueba primero la reproducción de uno de los MP4 ya descargados, fuera de AniLinux:

```bash
LIBGL_ALWAYS_SOFTWARE=1 mpv --no-config --hwdec=no \
  --vo=gpu --gpu-api=opengl --gpu-sw=yes -- "/ruta/al/episodio.mp4"
```

Después ejecuta la aplicación desde el terminal:

```bash
LIBGL_ALWAYS_SOFTWARE=1 ./target/release/anilinux 2>&1 | tee anilinux-reproduccion.log
```

El registro permite distinguir resolución del servidor, errores HTTP, argumentos del reproductor y problemas gráficos. Antes de compartirlo revisa si contiene enlaces privados o temporales del servidor. No publiques cookies o credenciales.

## Validación de esta revisión

- 18 pruebas normales aprobadas y Clippy sin advertencias.
- 3 pruebas adicionales con un mpv real 0.40.0 en X11 virtual: vídeo local, vídeo servido por HTTP y fallo HTTP 404. Se activaron explícitamente, no se cuentan como pruebas normales que no necesitan programas externos.
- Vídeo de prueba sintético, sin contenido de terceros ni audio. No se ha repetido una prueba completa de todos los alojamientos de AnimeAV1 ni se ha probado el paquete exacto 0.37.0 de Ubuntu del usuario.
- Se comprueba el comienzo de reproducción; no se garantiza que una conexión externa no falle más adelante.

### Repetir las pruebas opcionales de mpv

En una sesión gráfica con mpv y FFmpeg instalados, genera un archivo sintético:

```bash
mkdir -p /tmp/anilinux-player-test
ffmpeg -f lavfi -i 'testsrc2=size=320x180:rate=24' -t 4 \
  -c:v mpeg4 -q:v 7 -movflags +faststart -an -y /tmp/anilinux-player-test/test.mp4
```

En otro terminal sirve únicamente esa carpeta de prueba:

```bash
python3 -m http.server 8765 --bind 127.0.0.1 --directory /tmp/anilinux-player-test
```

Ejecuta desde el proyecto:

```bash
LIBGL_ALWAYS_SOFTWARE=1 \
ANILINUX_TEST_VIDEO=/tmp/anilinux-player-test/test.mp4 \
ANILINUX_TEST_STREAM=http://127.0.0.1:8765/test.mp4 \
cargo test --locked --test player_integration -- --ignored --test-threads=1 --nocapture
```

El servidor local es solo para estas pruebas de desarrollo: AniLinux no necesita ningún servidor local para funcionar. Detén el servidor al terminar.
