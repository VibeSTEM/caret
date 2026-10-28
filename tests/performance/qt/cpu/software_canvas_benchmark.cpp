// 无头测量 Caret 画布的 Qt Quick 软件场景图 CPU 路径。
#include "canvas_fixture.hpp"
#include <QElapsedTimer>
#include <QGuiApplication>
#include <QImage>
#include <QQuickRenderControl>
#include <QQuickRenderTarget>
#include <QQuickWindow>
#include <QSGRendererInterface>
#include <algorithm>
#include <cmath>
#include <cstdio>
#include <cstdlib>
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
    std::sort(values.begin(), values.end());
    const auto index = static_cast<std::size_t>(std::ceil(fraction * values.size())) - 1;
    return values[std::min(index, values.size() - 1)];
}

int run(int samples, int page_count, bool ime_suffix_shift) {
    using namespace caret::performance;
    QQuickWindow::setGraphicsApi(QSGRendererInterface::Software);
    QQuickRenderControl render_control;
    QQuickWindow window(&render_control);
    window.resize(render_size);
    QImage target(render_size, QImage::Format_RGBA8888_Premultiplied);
    target.fill(Qt::white);
    window.setRenderTarget(QQuickRenderTarget::fromPaintDevice(&target));
    caret::DocumentCanvas canvas;
    set_up_canvas(&canvas, page_count);
    const int visible_page = page_count == 1 ? 0 : page_count / 2;
    canvas.setParentItem(window.contentItem());
    const auto render_frame = [&render_control]() {
        render_control.polishItems();
        if (!render_control.sync()) {
            return false;
        }
        render_control.render();
        return true;
    };
    for (int frame = 0; frame < warmup_frames; ++frame) {
        const QVariantMap glyph_frame = ime_suffix_shift
                                            ? ime_updated_glyph_frame(visible_page, frame)
                                            : edited_glyph_frame(visible_page, frame);
        const QVariantList selection_frame = selection_rectangles(frame + 1, visible_page);
        canvas.setGlyphFrames({glyph_frame});
        canvas.setSelectionRects(selection_frame);
        if (!render_frame()) {
            std::fprintf(stderr, "QQuickRenderControl 软件渲染同步失败\n");
            return 2;
        }
    }
    std::vector<double> samples_us;
    std::vector<double> install_samples_us;
    std::vector<double> render_samples_us;
    samples_us.reserve(static_cast<std::size_t>(samples));
    install_samples_us.reserve(static_cast<std::size_t>(samples));
    render_samples_us.reserve(static_cast<std::size_t>(samples));
    for (int frame = 0; frame < samples; ++frame) {
        const int frame_index = frame + warmup_frames;
        const QVariantMap glyph_frame = ime_suffix_shift
                                            ? ime_updated_glyph_frame(visible_page, frame_index)
                                            : edited_glyph_frame(visible_page, frame_index);
        const QVariantList selection_frame =
            selection_rectangles(frame + warmup_frames, visible_page);
        QElapsedTimer timer;
        timer.start();
        canvas.setGlyphFrames({glyph_frame});
        canvas.setSelectionRects(selection_frame);
        install_samples_us.push_back(static_cast<double>(timer.nsecsElapsed()) / 1000.0);
        timer.restart();
        if (!render_frame()) {
            std::fprintf(stderr, "QQuickRenderControl 软件渲染同步失败\n");
            return 2;
        }
        const double render_time_us = static_cast<double>(timer.nsecsElapsed()) / 1000.0;
        render_samples_us.push_back(render_time_us);
        samples_us.push_back(install_samples_us.back() + render_time_us);
    }
    std::printf("{\"suite\":\"qt/cpu/software_scene_graph\",\"api\":\"Software\","
                "\"renderer\":\"Qt Quick software adaptation\",\"device_type\":\"cpu_renderer\","
                "\"qt_version\":\"%s\",\"font_source\":\"synthetic_rgba8_glyph_images\","
                "\"timed_path\":\"setGlyphFrames+setSelectionRects+polish_sync_render\","
                "\"update_scenario\":\"%s\","
                "\"samples\":%d,\"resolution\":\"%dx%d\",\"page_count\":%d,"
                "\"glyph_placements\":%d,"
                "\"unique_images\":%d,\"selection_rectangles\":%d,"
                "\"median_install_cpu_us\":%.3f,\"median_scene_graph_cpu_us\":%.3f,"
                "\"median_frame_cpu_us\":%.3f,\"p95_frame_cpu_us\":%.3f}\n",
                qVersion(), ime_suffix_shift ? "ime_suffix_shift" : "single_glyph_change", samples,
                render_size.width(), render_size.height(), page_count, glyph_placements,
                unique_glyph_images, selection_rectangles_count,
                percentile(install_samples_us, 0.50), percentile(render_samples_us, 0.50),
                percentile(samples_us, 0.50), percentile(samples_us, 0.95));
    render_control.invalidate();
    return 0;
}
} // namespace

int main(int argc, char** argv) {
    if (argc < 2 || std::string(argv[1]) != "cpu") {
        std::fprintf(stderr, "用法：caret_qt_cpu_benchmark cpu [--samples N]\n");
        return 1;
    }
    const int samples = parse_samples(argc, argv);
    if (samples < 10) {
        std::fprintf(stderr, "--samples 必须是 10 到 10000 之间的整数\n");
        return 1;
    }
    QGuiApplication application(argc, argv);
    for (const bool ime_suffix_shift : {false, true}) {
        for (const int page_count : {1, caret::performance::novel_page_count}) {
            const int result = run(samples, page_count, ime_suffix_shift);
            if (result != 0) {
                return result;
            }
        }
    }
    return 0;
}
