package dev.yzewe.rynet.ui

import android.Manifest
import android.annotation.SuppressLint
import android.content.Context
import android.content.pm.PackageManager
import android.graphics.Bitmap
import android.graphics.BitmapFactory
import android.graphics.ImageFormat
import android.hardware.camera2.CameraCaptureSession
import android.hardware.camera2.CameraCharacteristics
import android.hardware.camera2.CameraDevice
import android.hardware.camera2.CameraManager
import android.hardware.camera2.CaptureRequest
import android.media.ImageReader
import android.net.Uri
import android.os.Handler
import android.os.HandlerThread
import android.os.Looper
import android.util.Size
import android.view.SurfaceHolder
import android.view.SurfaceView
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.PickVisualMediaRequest
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.aspectRatio
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.safeDrawingPadding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.rounded.Close
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.ExperimentalMaterial3ExpressiveApi
import androidx.compose.material3.FilledTonalButton
import androidx.compose.material3.FilledTonalIconButton
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButtonDefaults
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import androidx.compose.ui.viewinterop.AndroidView
import androidx.compose.ui.window.Dialog
import androidx.compose.ui.window.DialogProperties
import androidx.core.content.ContextCompat
import com.google.zxing.BarcodeFormat
import com.google.zxing.BinaryBitmap
import com.google.zxing.DecodeHintType
import com.google.zxing.LuminanceSource
import com.google.zxing.PlanarYUVLuminanceSource
import com.google.zxing.RGBLuminanceSource
import com.google.zxing.common.GlobalHistogramBinarizer
import com.google.zxing.common.HybridBinarizer
import com.google.zxing.qrcode.QRCodeReader
import dev.yzewe.rynet.tr
import java.util.concurrent.atomic.AtomicBoolean

@OptIn(ExperimentalMaterial3ExpressiveApi::class)
@Composable
fun QrScanDialog(
    onResult: (String) -> Unit,
    onDismiss: () -> Unit,
    title: String = tr("QR-код подписки", "Subscription QR code"),
    keep: (String) -> Boolean = { false },
    progress: String? = null
) {
    val ctx = LocalContext.current
    var granted by remember { mutableStateOf(ContextCompat.checkSelfPermission(ctx, Manifest.permission.CAMERA) == PackageManager.PERMISSION_GRANTED) }
    var asked by remember { mutableStateOf(false) }
    var note by remember { mutableStateOf<String?>(null) }
    val permission = rememberLauncherForActivityResult(ActivityResultContracts.RequestPermission()) { ok ->
        granted = ok
        asked = true
    }
    LaunchedEffect(Unit) { if (!granted) permission.launch(Manifest.permission.CAMERA) }
    val pick = rememberLauncherForActivityResult(ActivityResultContracts.PickVisualMedia()) { uri ->
        if (uri != null) {
            val text = QrDecode.fromImage(ctx, uri)
            if (text != null) onResult(text) else note = tr("На картинке не нашёлся QR-код", "No QR code found in the image")
        }
    }
    Dialog(onDismiss, DialogProperties(usePlatformDefaultWidth = false, decorFitsSystemWindows = false)) {
        Box(Modifier.fillMaxSize().background(Color.Black)) {
            if (granted) {
                CameraPreview({ t ->
                    if (keep(t)) {
                        false
                    } else {
                        onResult(t)
                        true
                    }
                }, onError = { note = it })
            }
            Box(
                Modifier.align(Alignment.Center).fillMaxWidth(0.72f).aspectRatio(1f)
                    .border(4.dp, Color.White.copy(alpha = 0.9f), MaterialTheme.shapes.extraLargeIncreased)
            )
            Column(Modifier.fillMaxSize().safeDrawingPadding().padding(16.dp)) {
                Row(verticalAlignment = Alignment.CenterVertically) {
                    FilledTonalIconButton(
                        onClick = onDismiss,
                        shapes = IconButtonDefaults.shapes(),
                        colors = IconButtonDefaults.filledTonalIconButtonColors(containerColor = Color.Black.copy(alpha = 0.5f), contentColor = Color.White),
                        modifier = Modifier.size(48.dp)
                    ) { Icon(Icons.Rounded.Close, tr("Закрыть", "Close")) }
                    Spacer(Modifier.width(12.dp))
                    Text(title, color = Color.White, style = MaterialTheme.typography.titleLarge)
                }
                Spacer(Modifier.weight(1f))
                val msg = progress ?: note ?: if (!granted && asked) tr("Нет доступа к камере — можно выбрать фото с QR-кодом", "No camera access — you can pick a photo with a QR code") else null
                msg?.let {
                    Text(
                        it,
                        color = Color.White,
                        style = MaterialTheme.typography.bodyLarge,
                        textAlign = TextAlign.Center,
                        modifier = Modifier.fillMaxWidth().clip(MaterialTheme.shapes.extraLarge).background(Color.Black.copy(alpha = 0.55f)).padding(14.dp)
                    )
                    Spacer(Modifier.height(12.dp))
                }
                FilledTonalButton(
                    onClick = { pick.launch(PickVisualMediaRequest(ActivityResultContracts.PickVisualMedia.ImageOnly)) },
                    shapes = ButtonDefaults.shapes(),
                    modifier = Modifier.fillMaxWidth().height(ButtonDefaults.MediumContainerHeight)
                ) {
                    Icon(Glyph.Wallpaper, null, Modifier.size(ButtonDefaults.MediumIconSize))
                    Spacer(Modifier.width(8.dp))
                    Text(tr("Из фото", "From photo"), style = MaterialTheme.typography.titleMedium)
                }
            }
        }
    }
}

@SuppressLint("MissingPermission")
@Composable
private fun CameraPreview(onResult: (String) -> Boolean, onError: (String) -> Unit) {
    val ctx = LocalContext.current
    val scanner = remember { CameraScanner(ctx, onResult, onError) }
    val aspect = scanner.size?.let { it.height.toFloat() / it.width } ?: (3f / 4f)
    DisposableEffect(Unit) { onDispose { scanner.release() } }
    Box(Modifier.fillMaxSize(), contentAlignment = Alignment.Center) {
        AndroidView(
            factory = { c ->
                SurfaceView(c).apply {
                    holder.addCallback(object : SurfaceHolder.Callback {
                        override fun surfaceCreated(h: SurfaceHolder) = scanner.open(h)
                        override fun surfaceChanged(h: SurfaceHolder, f: Int, w: Int, hh: Int) {}
                        override fun surfaceDestroyed(h: SurfaceHolder) = scanner.close()
                    })
                }
            },
            modifier = Modifier.fillMaxHeight().aspectRatio(aspect, matchHeightConstraintsFirst = true)
        )
    }
}

private class CameraScanner(private val ctx: Context, private val onResult: (String) -> Boolean, private val onError: (String) -> Unit) {
    private val manager = ctx.getSystemService(CameraManager::class.java)
    private val thread = HandlerThread("rynet-qr").apply { start() }
    private val bg = Handler(thread.looper)
    private val main = Handler(Looper.getMainLooper())
    private val busy = AtomicBoolean(false)
    private val done = AtomicBoolean(false)
    private var device: CameraDevice? = null
    private var session: CameraCaptureSession? = null
    private var reader: ImageReader? = null
    private val id: String? = runCatching {
        manager.cameraIdList.firstOrNull { manager.getCameraCharacteristics(it).get(CameraCharacteristics.LENS_FACING) == CameraCharacteristics.LENS_FACING_BACK }
            ?: manager.cameraIdList.firstOrNull()
    }.getOrNull()

    val size: Size? = id?.let { cam ->
        runCatching {
            val map = manager.getCameraCharacteristics(cam).get(CameraCharacteristics.SCALER_STREAM_CONFIGURATION_MAP) ?: return@runCatching null
            val yuv = map.getOutputSizes(ImageFormat.YUV_420_888).toSet()
            map.getOutputSizes(SurfaceHolder::class.java).filter { it in yuv && it.width <= 1920 }
                .minByOrNull { kotlin.math.abs(it.width * it.height - 1280 * 720) }
        }.getOrNull()
    }

    @SuppressLint("MissingPermission")
    fun open(holder: SurfaceHolder) {
        val cam = id
        val sz = size
        if (cam == null || sz == null) {
            onError(tr("Камера недоступна — можно выбрать фото с QR-кодом", "Camera unavailable — you can pick a photo with a QR code"))
            return
        }
        holder.setFixedSize(sz.width, sz.height)
        val r = ImageReader.newInstance(sz.width, sz.height, ImageFormat.YUV_420_888, 2)
        reader = r
        r.setOnImageAvailableListener({ ir ->
            val img = ir.acquireLatestImage() ?: return@setOnImageAvailableListener
            try {
                if (done.get() || !busy.compareAndSet(false, true)) return@setOnImageAvailableListener
                val plane = img.planes[0]
                val buf = plane.buffer
                val bytes = ByteArray(buf.remaining()).also { buf.get(it) }
                val src = PlanarYUVLuminanceSource(bytes, plane.rowStride, img.height, 0, 0, img.width, img.height, false)
                QrDecode.decode(src)?.let { text ->
                    main.post { if (!done.get() && onResult(text)) done.set(true) }
                }
            } finally {
                img.close()
                busy.set(false)
            }
        }, bg)
        runCatching {
            manager.openCamera(
                cam,
                object : CameraDevice.StateCallback() {
                    override fun onOpened(d: CameraDevice) {
                        device = d
                        val targets = listOf(holder.surface, r.surface)
                        @Suppress("DEPRECATION")
                        d.createCaptureSession(
                            targets,
                            object : CameraCaptureSession.StateCallback() {
                                override fun onConfigured(s: CameraCaptureSession) {
                                    session = s
                                    val req = d.createCaptureRequest(CameraDevice.TEMPLATE_PREVIEW).apply {
                                        targets.forEach { addTarget(it) }
                                        set(CaptureRequest.CONTROL_AF_MODE, CaptureRequest.CONTROL_AF_MODE_CONTINUOUS_PICTURE)
                                    }.build()
                                    runCatching { s.setRepeatingRequest(req, null, bg) }
                                }
                                override fun onConfigureFailed(s: CameraCaptureSession) = main.post { onError(tr("Камера не запустилась", "Camera failed to start")) }.let { }
                            },
                            bg
                        )
                    }
                    override fun onDisconnected(d: CameraDevice) = d.close()
                    override fun onError(d: CameraDevice, e: Int) {
                        d.close()
                        main.post { onError(tr("Камера занята или недоступна", "Camera is busy or unavailable")) }
                    }
                },
                bg
            )
        }.onFailure { onError(tr("Камера недоступна: ${it.message.orEmpty()}", "Camera unavailable: ${it.message.orEmpty()}")) }
    }

    fun close() {
        runCatching { session?.close() }
        runCatching { device?.close() }
        runCatching { reader?.close() }
        session = null
        device = null
        reader = null
    }

    fun release() {
        close()
        thread.quitSafely()
    }
}

object QrDecode {
    private val hints = mapOf(DecodeHintType.POSSIBLE_FORMATS to listOf(BarcodeFormat.QR_CODE), DecodeHintType.TRY_HARDER to true)

    fun decode(src: LuminanceSource): String? {
        val reader = QRCodeReader()
        for (bin in listOf(HybridBinarizer(src), GlobalHistogramBinarizer(src))) {
            runCatching { return reader.decode(BinaryBitmap(bin), hints).text }
            reader.reset()
        }
        runCatching { return reader.decode(BinaryBitmap(HybridBinarizer(src.invert())), hints).text }
        return null
    }

    fun fromImage(ctx: Context, uri: Uri): String? = runCatching {
        val bounds = BitmapFactory.Options().apply { inJustDecodeBounds = true }
        ctx.contentResolver.openInputStream(uri)?.use { BitmapFactory.decodeStream(it, null, bounds) }
        var sample = 1
        while (maxOf(bounds.outWidth, bounds.outHeight) / sample > 1600) sample *= 2
        val bmp: Bitmap = ctx.contentResolver.openInputStream(uri)?.use {
            BitmapFactory.decodeStream(it, null, BitmapFactory.Options().apply { inSampleSize = sample })
        } ?: return null
        val px = IntArray(bmp.width * bmp.height)
        bmp.getPixels(px, 0, bmp.width, 0, 0, bmp.width, bmp.height)
        decode(RGBLuminanceSource(bmp.width, bmp.height, px))
    }.getOrNull()
}
