package com.soubhagyajit.awa.server

import android.util.Log
import com.soubhagyajit.awa.viewModel.CameraViewModel
import io.ktor.http.*
import io.ktor.serialization.kotlinx.json.json
import io.ktor.server.application.*
import io.ktor.server.engine.*
import io.ktor.server.cio.*
import io.ktor.server.http.content.staticResources
import io.ktor.server.plugins.contentnegotiation.ContentNegotiation
import io.ktor.server.plugins.cors.routing.CORS
import io.ktor.server.websocket.WebSockets
import io.ktor.server.websocket.webSocket
import io.ktor.server.request.receive
import io.ktor.server.response.*
import io.ktor.server.routing.*
import io.ktor.websocket.Frame
import io.ktor.websocket.send
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.delay
import kotlinx.coroutines.withContext
import kotlinx.serialization.Serializable
import kotlinx.serialization.json.Json
import java.net.ServerSocket
import java.net.DatagramPacket
import java.net.DatagramSocket
import java.io.OutputStream
import java.util.concurrent.atomic.AtomicBoolean

object VideoStreamServer {

    const val HTTP_PORT = 4848

    @Serializable
    data class FeaturesResponse(
        val resolutions: List<String>,
        val manual_focus: Boolean,
        val exposure_lower: Int,
        val exposure_upper: Int,
        val has_zoom: Boolean,
        val zoom_max: Float,
        val zoom_min: Float,
        val stream_protocol: CameraViewModel.StreamMode,
        val websocket_available: Boolean = false,
        val server_port: Int? = null,
        val rtsp_port: Int? = null,
        val rotation: Int = 0,
        val mirror: Boolean = false
    )

    @Serializable
    data class SettingsResponse(
        val camera: String,
        val resolution_str: String,
        val zoom: Float,
        val focus_mode: Int,
        val focus_distance: Float,
        val exposure_index: Int,
        val autofocus: Boolean? = null,
        val stream_quality: Int,
        val flash: Boolean = false,
        val has_flash_unit: Boolean? = null,
        val rotation: Int = 0,
        val mirror: Boolean = false,
    )

    @Serializable
    data class SettingsUpdateRequest(
        val focus_mode: Int? = null,
        val focus_distance: Float? = null,
        val autofocus: Boolean? = null,
        val exposure_index: Int? = null,
        val zoom: Float? = null,
        val flash: Boolean? = null,
        val resolution_str: String? = null,
        val switchCamera: Boolean? = null,
        val camera: String,
        val stream_quality: Int? = null,
        val rotation: Int? = null,
        val mirror: Boolean? = null,
    )

    var featuresProvider: (() -> FeaturesResponse)? = null
    var settingsProvider: (() -> SettingsResponse)? = null

    // Returns null on success, or an error message String if rejected
    var onSettingsUpdated: ((SettingsUpdateRequest) -> String?)? = null

    private var server: EmbeddedServer<*, *>? = null
    private var discoverySocket: DatagramSocket? = null
    private var discoveryThread: Thread? = null
    private val discoveryRunning = AtomicBoolean(false)
    @Volatile
    var latestFrame: ByteArray? = null

    var onUserConnected: (() -> Unit)? = null
    var onUserDisconnected: (() -> Unit)? = null
    var onServerStateChanged: ((Boolean) -> Unit)? = null

    val isRunning: Boolean
        get() = server != null

    fun toggleServer(port: Int = HTTP_PORT) {
        if (isRunning) {
            stop()
        } else {
            start(port)
        }
        onServerStateChanged?.invoke(isRunning)
    }

    fun start(port: Int = HTTP_PORT) {
        if (server != null) return

        if (!isPortAvailable(port)) {
            Log.e("AWA", "Cannot start HTTP server: port $port is already in use")
            onServerStateChanged?.invoke(false)
            return
        }

        server = embeddedServer(CIO, port = port) {
            install(ContentNegotiation) {
                json(
                    Json {
                        prettyPrint = true
                        ignoreUnknownKeys = true
                    }
                )
            }
            install(CORS) {
                anyHost()
                allowMethod(HttpMethod.Get)
                allowMethod(HttpMethod.Post)
                allowHeader(HttpHeaders.ContentType)
            }
            install(WebSockets)
            routing {
                get("/video") {
                    onUserConnected?.invoke()
                    try {
                        call.respondOutputStream(
                            contentType = ContentType.parse("multipart/x-mixed-replace; boundary=--frame"),
                            status = HttpStatusCode.OK
                        ) {
                            streamMjpeg(this)
                        }
                    } finally {
                        onUserDisconnected?.invoke()
                    }
                }

                // A lightweight binary JPEG stream for desktop clients. It uses
                // the same HTTP port as MJPEG, but avoids multipart parsing.
                webSocket("/stream") {
                    onUserConnected?.invoke()
                    try {
                        while (true) {
                            latestFrame?.let { send(Frame.Binary(true, it)) }
                            delay(33)
                        }
                    } finally {
                        onUserDisconnected?.invoke()
                    }
                }

                get("/features") {
                    val response = featuresProvider?.invoke() ?: FeaturesResponse(
                        resolutions = listOf("640x480", "1280x720", "1920x1080"),
                        manual_focus = false,
                        exposure_upper = 0,
                        exposure_lower = 0,
                        has_zoom = false,
                        zoom_max = 1.0f,
                        zoom_min = 1.0f,
                        stream_protocol = CameraViewModel.StreamMode.MJPEG,
                        server_port = null,
                        rtsp_port = null,
                        rotation = 0,
                        mirror = false
                    )
                    call.respond(response)
                }

                get("/settings") {
                    val response = settingsProvider?.invoke() ?: SettingsResponse(
                        camera = "back",
                        resolution_str = "1280x720",
                        zoom = 1.0f,
                        flash = false,
                        exposure_index = 1,
                        autofocus = true,
                        focus_mode = 0,
                        focus_distance = 0f,
                        has_flash_unit = false,
                        stream_quality = 80,
                        rotation = 0,
                        mirror = false,
                    )
                    call.respond(response)
                }

                // POST Endpoint
                post("/settings") {
                    try {
                        val request = call.receive<SettingsUpdateRequest>()
                        val error = onSettingsUpdated?.invoke(request)
                        if (error != null) {
                            call.respond(HttpStatusCode.BadRequest, mapOf("error" to error))
                        } else {
                            call.respond(HttpStatusCode.OK, mapOf("status" to "success"))
                        }
                    } catch (e: Exception) {
                        call.respond(HttpStatusCode.BadRequest, mapOf("error" to (e.message ?: "Invalid payload")))
                    }
                }

                // GET Endpoint
                get("/control") {
                    val params = call.request.queryParameters
                    Log.d("AWA", "$params")
                    val update = SettingsUpdateRequest(
                        camera = params["camera"] ?: "back",
                        resolution_str = params["resolution_str"],
                        zoom = params["zoom"]?.toFloatOrNull(),
                        flash = params["flash"]?.toBooleanStrictOrNull(),
                        exposure_index = params["exposure_index"]?.toIntOrNull(),
                        autofocus = params["autofocus"]?.toBooleanStrictOrNull(),
                        focus_mode = params["focus_mode"]?.toIntOrNull(),
                        focus_distance = params["focus_distance"]?.toFloatOrNull(),
                        switchCamera = if (params.contains("switch_camera")) true else null,
                        stream_quality = 80,
                        rotation = params["rotation"]?.toIntOrNull(),
                        mirror = params["mirror"]?.toBooleanStrictOrNull(),
                    )

                    val error = onSettingsUpdated?.invoke(update)
                    if (error != null) {
                        call.respond(HttpStatusCode.BadRequest, mapOf("error" to error))
                    } else {
                        val response = settingsProvider?.invoke() ?: SettingsResponse(
                            focus_mode = 0, focus_distance = 0f, exposure_index = 1,
                            zoom = 1.0f, stream_quality = 80, resolution_str = "1280x720",
                            camera = "back"
                        )
                        call.respond(HttpStatusCode.OK, response)
                    }
                }
                staticResources("/static", "static")
                get("/help"){
                    call.respondRedirect("/static/help.html")
                }

            }
        }.start(wait = false)
        startDiscovery()
    }

    private fun startDiscovery() {
        if (!discoveryRunning.compareAndSet(false, true)) return
        discoveryThread = Thread {
            try {
                DatagramSocket(HTTP_PORT).also { discoverySocket = it }.use { socket ->
                    socket.broadcast = true
                    val buffer = ByteArray(128)
                    while (discoveryRunning.get()) {
                        try {
                            val packet = DatagramPacket(buffer, buffer.size)
                            socket.receive(packet)
                            val request = String(packet.data, packet.offset, packet.length)
                            if (request.startsWith("AWC_DISCOVER")) {
                                val response = "AWC_DISCOVERY_REPLY|$HTTP_PORT".toByteArray()
                                socket.send(DatagramPacket(response, response.size, packet.address, packet.port))
                            }
                        } catch (_: Exception) {
                            if (discoveryRunning.get()) Log.w("AWA", "Discovery request failed")
                        }
                    }
                }
            } catch (e: Exception) {
                Log.w("AWA", "UDP discovery unavailable", e)
            } finally {
                discoverySocket = null
                discoveryRunning.set(false)
            }
        }.also { it.name = "awc-discovery"; it.isDaemon = true; it.start() }
    }

    private fun isPortAvailable(port: Int): Boolean {
        return try {
            ServerSocket(port).use { true }
        } catch (e: Exception) {
            Log.e("AWA", "Port check failed for $port", e)
            false
        }
    }

    private suspend fun streamMjpeg(outputStream: OutputStream) {
        val boundary = "\r\n--frame\r\n"

        while (true) {
            val frame = latestFrame
            if (frame != null) {
                try {
                    withContext(Dispatchers.IO) {
                        outputStream.write(boundary.toByteArray())
                        outputStream.write("Content-Type: image/jpeg\r\n".toByteArray())
                        outputStream.write("Content-Length: ${frame.size}\r\n\r\n".toByteArray())
                        outputStream.write(frame)
                        outputStream.flush()
                    }
                } catch (_: Exception) {
                    break
                }
            }
            delay(33)
        }
    }

    fun stop() {
        discoveryRunning.set(false)
        discoverySocket?.close()
        discoveryThread = null
        server?.stop(1000, 2000)
        server = null
    }
}
