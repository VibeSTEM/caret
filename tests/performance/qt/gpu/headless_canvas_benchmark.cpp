// 无头测量 Caret 画布的 Qt Quick RHI GPU 渲染路径。
#include "canvas_fixture.hpp"
#include <QElapsedTimer>
#include <QGuiApplication>
#include <QQuickGraphicsConfiguration>
#include <QQuickRenderControl>
#include <QQuickRenderTarget>
#include <QQuickWindow>
#include <QSGRendererInterface>
#include <algorithm>
#include <cmath>
#include <cstdio>
#include <cstdlib>
#include <memory>
#include <rhi/qrhi.h>
#include <string>
#include <vector>

namespace {
int parse_samples(int argc, char** argv) {
    if (argc == 2) {
        return caret::performance::default_samples;
    }
    if (argc != 4 || std::string(argv[2]) != "--samples") {
        return -1;
    }
    char* end = nullptr;
    const long value = std::strtol(argv[3], &end, 10);
    if (end == argv[3] || *end != '\0' || value < 10 || value > 10000) {
        return -1;
    }
    return static_cast<int>(value);
}

double percentile(std::vector<double> values, double fraction) {
    if (values.empty()) {
        return 0.0;
    }
    std::sort(values.begin(), values.end());
    const auto index = static_cast<std::size_t>(std::ceil(fraction * values.size())) - 1;
    return values[std::min(index, values.size() - 1)];
}

QString device_type_name(QRhi* rhi) {
    switch (rhi->driverInfo().deviceType) {
    case QRhiDriverInfo::IntegratedDevice:
        return QStringLiteral("integrated_gpu");
    case QRhiDriverInfo::DiscreteDevice:
        return QStringLiteral("discrete_gpu");
    case QRhiDriverInfo::ExternalDevice:
        return QStringLiteral("external_gpu");
    case QRhiDriverInfo::VirtualDevice:
        return QStringLiteral("virtual_gpu");
    case QRhiDriverInfo::CpuDevice:
        return QStringLiteral("cpu_renderer");
    case QRhiDriverInfo::UnknownDevice:
        return QStringLiteral("unknown");
    }
    return QStringLiteral("unknown");
}

int run(int samples) {
    using namespace caret::performance;
    QQuickRenderControl render_control;
    QQuickWindow window(&render_control);
    window.resize(render_size);
    QQuickGraphicsConfiguration graphics_configuration;
    graphics_configuration.setTimestamps(true);
    window.setGraphicsConfiguration(graphics_configuration);
    caret::DocumentCanvas canvas;
    set_up_canvas(&canvas);
    canvas.setParentItem(window.contentItem());
    if (!render_control.initialize()) {
        std::fprintf(stderr, "QQuickRenderControl OpenGL 初始化失败\n");
        return 2;
    }
    QRhi* rhi = render_control.rhi();
    if (rhi == nullptr) {
        std::fprintf(stderr, "QQuickRenderControl 未创建 RHI\n");
        return 2;
    }
    std::unique_ptr<QRhiTexture> texture(
        rhi->newTexture(QRhiTexture::RGBA8, render_size, 1,
                        QRhiTexture::RenderTarget | QRhiTexture::UsedAsTransferSource));
    std::unique_ptr<QRhiRenderBuffer> depth_stencil(
        rhi->newRenderBuffer(QRhiRenderBuffer::DepthStencil, render_size, 1));
    if (!texture->create() || !depth_stencil->create()) {
        std::fprintf(stderr, "无法创建 GPU 离屏目标\n");
        return 2;
    }
    QRhiTextureRenderTargetDescription target_description(QRhiColorAttachment(texture.get()));
    target_description.setDepthStencilBuffer(depth_stencil.get());
    std::unique_ptr<QRhiTextureRenderTarget> render_target(
        rhi->newTextureRenderTarget(target_description));
    std::unique_ptr<QRhiRenderPassDescriptor> render_pass(
        render_target->newCompatibleRenderPassDescriptor());
    render_target->setRenderPassDescriptor(render_pass.get());
    if (!render_target->create()) {
        std::fprintf(stderr, "无法初始化 GPU 离屏 render target\n");
        return 2;
    }
    window.setRenderTarget(QQuickRenderTarget::fromRhiRenderTarget(render_target.get()));

    const auto render_frame = [&render_control]() {
        render_control.polishItems();
        render_control.beginFrame();
        if (!render_control.sync()) {
            render_control.endFrame();
            return false;
        }
        render_control.render();
        render_control.endFrame();
        return true;
    };
    for (int frame = 0; frame < warmup_frames; ++frame) {
        canvas.setSelectionRects(selection_rectangles(frame + 1));
        if (!render_frame()) {
            std::fprintf(stderr, "QQuickRenderControl GPU 同步失败\n");
            return 2;
        }
    }
    const bool gpu_time_supported = rhi->isFeatureSupported(QRhi::Timestamps);
    std::vector<double> cpu_samples_us;
    std::vector<double> gpu_samples_us;
    cpu_samples_us.reserve(static_cast<std::size_t>(samples));
    gpu_samples_us.reserve(static_cast<std::size_t>(samples));
    for (int frame = 0; frame < samples; ++frame) {
        canvas.setSelectionRects(selection_rectangles(frame + warmup_frames));
        QElapsedTimer timer;
        timer.start();
        if (!render_frame()) {
            std::fprintf(stderr, "QQuickRenderControl GPU 同步失败\n");
            return 2;
        }
        cpu_samples_us.push_back(static_cast<double>(timer.nsecsElapsed()) / 1000.0);
        if (gpu_time_supported) {
            const double gpu_time_us =
                render_control.commandBuffer()->lastCompletedGpuTime() * 1'000'000.0;
            if (std::isfinite(gpu_time_us) && gpu_time_us > 0.0) {
                gpu_samples_us.push_back(gpu_time_us);
            }
        }
    }
    const QByteArray renderer_name = rhi->driverInfo().deviceName;
    const QByteArray backend_name = QRhi::backendName(rhi->backend());
    std::printf("{\"suite\":\"qt/gpu/headless_document_canvas\",\"api\":\"%s\","
                "\"renderer\":\"%s\",\"device_type\":\"%s\",\"qt_version\":\"%s\","
                "\"font_source\":\"synthetic_rgba8_glyph_images\",\"samples\":%d,"
                "\"resolution\":\"%dx%d\",\"glyph_placements\":%d,\"unique_images\":%d,"
                "\"selection_rectangles\":%d,\"median_frame_cpu_us\":%.3f,"
                "\"p95_frame_cpu_us\":%.3f,\"gpu_time_supported\":%s,"
                "\"gpu_timestamp_samples\":%zu,\"gpu_timestamp_available\":%s,"
                "\"median_gpu_us\":%s,\"p95_gpu_us\":%s}\n",
                backend_name.constData(), renderer_name.constData(),
                device_type_name(rhi).toUtf8().constData(), qVersion(), samples,
                render_size.width(), render_size.height(), glyph_placements, unique_glyph_images,
                selection_rectangles_count, percentile(cpu_samples_us, 0.50),
                percentile(cpu_samples_us, 0.95), gpu_time_supported ? "true" : "false",
                gpu_samples_us.size(), gpu_samples_us.empty() ? "false" : "true",
                gpu_samples_us.empty()
                    ? "null"
                    : QByteArray::number(percentile(gpu_samples_us, 0.50), 'f', 3).constData(),
                gpu_samples_us.empty()
                    ? "null"
                    : QByteArray::number(percentile(gpu_samples_us, 0.95), 'f', 3).constData());
    render_control.invalidate();
    return 0;
}
} // namespace

int main(int argc, char** argv) {
    if (argc < 2 || std::string(argv[1]) != "gpu") {
        std::fprintf(stderr, "用法：caret_qt_gpu_benchmark gpu [--samples N]\n");
        return 1;
    }
    const int samples = parse_samples(argc, argv);
    if (samples < 10) {
        std::fprintf(stderr, "--samples 必须是 10 到 10000 之间的整数\n");
        return 1;
    }
    QQuickWindow::setGraphicsApi(QSGRendererInterface::OpenGL);
    QGuiApplication application(argc, argv);
    return run(samples);
}
