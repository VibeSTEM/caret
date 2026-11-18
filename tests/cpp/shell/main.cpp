// Shell QtTest 聚合入口，按类顺序运行或将单函数过滤分派到所属类。
#include "shell_document_autosave_test.hpp"
#include "shell_document_canvas_test.hpp"
#include "shell_document_close_test.hpp"
#include "shell_document_file_test.hpp"
#include "shell_document_interaction_test.hpp"
#include "shell_document_navigation_test.hpp"
#include "shell_document_path_test.hpp"
#include "shell_document_recovery_test.hpp"
#include "shell_layout_snapshot_validation_test.hpp"
#include "shell_load_test.hpp"
#include "shell_theme_catalog_test.hpp"
#include "shell_window_chrome_test.hpp"
#include <QtCore/qbytearray.h>
#include <QtCore/qchar.h>
#include <QtCore/qcontainerfwd.h>
#include <QtCore/qcoreapplication.h>
#include <QtCore/qfile.h>
#include <QtCore/qlist.h>
#include <QtCore/qmetaobject.h>
#include <QtCore/qobject.h>
#include <QtCore/qprocess.h>
#include <QtCore/qregularexpression.h>
#include <QtCore/qstring.h>
#include <QtCore/qstringlist.h>
#include <QtCore/qtemporarydir.h>
#include <QtCore/qtextstream.h>
#include <QtGui/qguiapplication.h>
#include <QtQml/qqmlextensionplugin.h>
#include <QtQuick/qquickwindow.h>
#include <QtTest/qtest.h>
#include <QtTest/qtestcase.h>
#include <algorithm>
#include <array>
#include <cstddef>
#include <cstdio>

Q_IMPORT_QML_PLUGIN(Caret_AdaptersPlugin)
Q_IMPORT_QML_PLUGIN(Caret_ShellPlugin)
Q_IMPORT_QML_PLUGIN(Caret_CanvasPlugin)
Q_IMPORT_QML_PLUGIN(Caret_ViewModelsPlugin)

namespace {

using TestClasses = std::array<QObject*, 12>;

bool optionConsumesValue(const QString& argument) {
    return argument == QStringLiteral("-o") || argument == QStringLiteral("-maxwarnings") ||
           argument == QStringLiteral("-eventdelay") || argument == QStringLiteral("-keydelay") ||
           argument == QStringLiteral("-mousedelay") || argument == QStringLiteral("-itemdelay") ||
           argument == QStringLiteral("-platform") || argument == QStringLiteral("-style") ||
           argument == QStringLiteral("-repeat") || argument == QStringLiteral("-minimumvalue") ||
           argument == QStringLiteral("-minimumtotal") ||
           argument == QStringLiteral("-iterations") || argument == QStringLiteral("-median");
}

QString legacyOutputFormat(const QString& argument) {
    if (argument == QStringLiteral("-txt"))
        return QStringLiteral("txt");
    if (argument == QStringLiteral("-csv"))
        return QStringLiteral("csv");
    if (argument == QStringLiteral("-xml"))
        return QStringLiteral("xml");
    if (argument == QStringLiteral("-lightxml"))
        return QStringLiteral("lightxml");
    if (argument == QStringLiteral("-junitxml"))
        return QStringLiteral("junitxml");
    if (argument == QStringLiteral("-tap"))
        return QStringLiteral("tap");
    if (argument == QStringLiteral("-teamcity"))
        return QStringLiteral("teamcity");
    return {};
}

QString outputFormat(const QString& output_argument) {
    const qsizetype comma = output_argument.lastIndexOf(QLatin1Char(','));
    if (comma < 0)
        return QStringLiteral("txt");
    return output_argument.mid(comma + 1);
}

bool outputFormatIsExplicit(const QString& output_argument) {
    return output_argument.contains(QLatin1Char(','));
}

QString outputPath(const QString& output_argument) {
    const qsizetype comma = output_argument.lastIndexOf(QLatin1Char(','));
    return comma < 0 ? output_argument : output_argument.left(comma);
}

bool hasFunction(const QObject* test, const QString& argument) {
    const QString function = argument.section(QLatin1Char(':'), 0, 0);
    const QByteArray signature = function.toLatin1() + QByteArrayLiteral("()");
    return test->metaObject()->indexOfSlot(signature.constData()) >= 0;
}

QList<int> selectedClasses(const QStringList& arguments, const TestClasses& tests) {
    std::array<bool, 12> matched{};
    for (qsizetype argument_index = 1; argument_index < arguments.size(); ++argument_index) {
        const QString& argument = arguments.at(argument_index);
        if (optionConsumesValue(argument)) {
            ++argument_index;
            continue;
        }
        if (argument.startsWith(QLatin1Char('-'))) {
            continue;
        }
        for (qsizetype test_index = 0; test_index < static_cast<qsizetype>(tests.size());
             ++test_index) {
            if (hasFunction(tests[static_cast<std::size_t>(test_index)], argument) &&
                !matched[static_cast<std::size_t>(test_index)]) {
                matched[static_cast<std::size_t>(test_index)] = true;
                break;
            }
        }
    }
    QList<int> selected;
    for (qsizetype index = 0; index < static_cast<qsizetype>(tests.size()); ++index) {
        if (matched[static_cast<std::size_t>(index)])
            selected.append(static_cast<int>(index));
    }
    return selected;
}

bool hasUnknownFilter(const QStringList& arguments, const TestClasses& tests) {
    for (qsizetype argument_index = 1; argument_index < arguments.size(); ++argument_index) {
        const QString& argument = arguments.at(argument_index);
        if (optionConsumesValue(argument)) {
            ++argument_index;
            continue;
        }
        if (argument.startsWith(QLatin1Char('-'))) {
            continue;
        }
        if (std::none_of(tests.cbegin(), tests.cend(), [&argument](const QObject* test) {
                return hasFunction(test, argument);
            })) {
            return true;
        }
    }
    return false;
}

QStringList argumentsForClass(const QStringList& arguments, int class_index,
                              const TestClasses& tests, const QList<int>& selected,
                              const QList<QString>& redirected_outputs,
                              const QList<qsizetype>& paired_legacy_positions) {
    if (selected.size() <= 1) {
        return arguments;
    }

    QStringList filtered{arguments.value(0)};
    qsizetype output_index = 0;
    for (qsizetype argument_index = 1; argument_index < arguments.size(); ++argument_index) {
        const QString& argument = arguments.at(argument_index);
        if (optionConsumesValue(argument)) {
            if (argument == QStringLiteral("-o") && argument_index + 1 < arguments.size() &&
                output_index < redirected_outputs.size()) {
                const QString& redirected = redirected_outputs.at(output_index++);
                ++argument_index;
                if (redirected.isEmpty())
                    continue;
                filtered.append(argument);
                filtered.append(redirected);
                continue;
            }
            filtered.append(argument);
            if (argument_index + 1 < arguments.size()) {
                const QString& value = arguments.at(++argument_index);
                filtered.append(value);
            }
            continue;
        }
        const QString legacy_format = legacyOutputFormat(argument);
        if (!legacy_format.isEmpty() && paired_legacy_positions.contains(argument_index))
            continue;
        if (!legacy_format.isEmpty() && output_index < redirected_outputs.size()) {
            filtered.append(QStringLiteral("-o"));
            filtered.append(redirected_outputs.at(output_index++));
            continue;
        }
        if (argument.startsWith(QLatin1Char('-'))) {
            filtered.append(argument);
            continue;
        }
        int matching_class = -1;
        for (qsizetype test_index = 0; test_index < static_cast<qsizetype>(tests.size());
             ++test_index) {
            if (hasFunction(tests[static_cast<std::size_t>(test_index)], argument)) {
                matching_class = static_cast<int>(test_index);
                break;
            }
        }
        if (matching_class == class_index ||
            (matching_class < 0 && selected.first() == class_index)) {
            filtered.append(argument);
        }
    }
    return filtered;
}

QByteArray mergeReports(const QList<QByteArray>& reports, const QString& format) {
    if (format == QStringLiteral("txt") || format == QStringLiteral("teamcity")) {
        QByteArray merged;
        for (const QByteArray& report : reports)
            merged.append(report);
        return merged;
    }
    if (format == QStringLiteral("tap")) {
        QByteArray merged;
        int test_index = 0;
        for (const QByteArray& report : reports) {
            const QList<QByteArray> lines = report.split('\n');
            for (QByteArray line : lines) {
                if (line.startsWith("TAP version ") || line.startsWith("1.."))
                    continue;
                if (line.startsWith("ok ") || line.startsWith("not ok ")) {
                    const qsizetype number_start = line.startsWith("not ok ") ? 7 : 3;
                    const qsizetype separator = line.indexOf(" - ", number_start);
                    if (separator > number_start) {
                        ++test_index;
                        line.replace(number_start, separator - number_start,
                                     QByteArray::number(test_index));
                    }
                }
                if (!line.isEmpty())
                    merged.append(line + '\n');
            }
        }
        const QByteArray header("TAP version 13\n");
        merged.prepend(header);
        merged.append("1..");
        merged.append(QByteArray::number(test_index));
        merged.append('\n');
        return merged;
    }
    if (format == QStringLiteral("xml") || format == QStringLiteral("lightxml")) {
        QByteArray environment;
        QByteArray functions;
        double elapsed_msecs = 0.0;
        const QRegularExpression environment_pattern(
            QStringLiteral("<Environment>[\\s\\S]*?</Environment>"));
        const QRegularExpression function_pattern(
            QStringLiteral("<TestFunction\\b[\\s\\S]*?</TestFunction>"));
        const QRegularExpression duration_pattern(
            QStringLiteral("<Duration msecs=\"([0-9.]+)\"/>"));
        for (const QByteArray& report : reports) {
            const QString xml = QString::fromUtf8(report);
            if (environment.isEmpty()) {
                const auto environment_match = environment_pattern.match(xml);
                if (environment_match.hasMatch())
                    environment = environment_match.captured().toUtf8();
            }
            auto function_match = function_pattern.globalMatch(xml);
            while (function_match.hasNext())
                functions.append(function_match.next().captured().toUtf8() + '\n');
            double report_duration = 0.0;
            auto duration_match = duration_pattern.globalMatch(xml);
            while (duration_match.hasNext())
                report_duration = duration_match.next().captured(1).toDouble();
            elapsed_msecs += report_duration;
        }
        QByteArray merged;
        if (format == QStringLiteral("xml")) {
            merged.append(
                "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<TestCase name=\"CaretShellTest\">\n");
        }
        merged.append(environment + '\n' + functions);
        merged.append("<Duration msecs=\"");
        merged.append(QByteArray::number(elapsed_msecs, 'f', 3));
        merged.append(format == QStringLiteral("xml") ? "\"/>\n</TestCase>\n" : "\"/>\n");
        return merged;
    }
    if (format == QStringLiteral("junitxml")) {
        QByteArray merged("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<testsuites>\n");
        const QRegularExpression suite_pattern(
            QStringLiteral("<testsuite\\b[\\s\\S]*?</testsuite>"));
        for (const QByteArray& report : reports) {
            const QString xml = QString::fromUtf8(report);
            auto suite_match = suite_pattern.globalMatch(xml);
            while (suite_match.hasNext())
                merged.append(suite_match.next().captured().toUtf8() + '\n');
        }
        merged.append("</testsuites>\n");
        return merged;
    }
    QByteArray merged;
    for (const QByteArray& report : reports)
        merged.append(report);
    return merged;
}

} // namespace

int main(int argc, char* argv[]) {
    QQuickWindow::setSceneGraphBackend(QStringLiteral("software"));
    const QGuiApplication application(argc, argv);
    const QStringList arguments = QCoreApplication::arguments();

    ShellLoadTest load_test;
    ShellDocumentPathTest path_test;
    ShellWindowChromeTest window_chrome_test;
    ShellDocumentCanvasTest canvas_test;
    ShellDocumentInteractionTest interaction_test;
    ShellDocumentFileTest file_test;
    ShellDocumentCloseTest close_test;
    ShellDocumentNavigationTest navigation_test;
    ShellLayoutSnapshotValidationTest snapshot_test;
    ShellDocumentAutoSaveTest autosave_test;
    ShellDocumentRecoveryTest recovery_test;
    ShellThemeCatalogTest theme_test;
    const TestClasses tests{&load_test,     &path_test,        &window_chrome_test,
                            &canvas_test,   &interaction_test, &file_test,
                            &close_test,    &navigation_test,  &snapshot_test,
                            &autosave_test, &recovery_test,    &theme_test};

    const qsizetype child_option = arguments.indexOf(QStringLiteral("--caret-test-class"));
    if (child_option >= 0 && child_option + 1 < arguments.size()) {
        bool ok = false;
        const int class_index = arguments.at(child_option + 1).toInt(&ok);
        if (!ok || class_index < 0 || class_index >= static_cast<int>(tests.size()))
            return 2;
        QStringList child_arguments = arguments;
        child_arguments.removeAt(child_option + 1);
        child_arguments.removeAt(child_option);
        return QTest::qExec(tests[static_cast<std::size_t>(class_index)], child_arguments);
    }

    if (arguments.contains(QStringLiteral("-functions"))) {
        QByteArray function_list;
        QTextStream output(&function_list, QIODevice::WriteOnly);
        for (const QObject* test : tests) {
            const QMetaObject* meta_object = test->metaObject();
            for (int method_index = meta_object->methodOffset();
                 method_index < meta_object->methodCount(); ++method_index) {
                const QMetaMethod method = meta_object->method(method_index);
                if (method.methodType() == QMetaMethod::Slot)
                    output << method.name() << "()\n";
            }
        }
        output.flush();
        QStringList output_targets;
        for (qsizetype index = 1; index + 1 < arguments.size(); ++index) {
            if (arguments.at(index) == QStringLiteral("-o"))
                output_targets.append(outputPath(arguments.at(++index)));
        }
        const QString destination =
            output_targets.isEmpty() ? QStringLiteral("-") : output_targets.last();
        if (destination == QStringLiteral("-")) {
            QTextStream(stdout) << function_list;
            return 0;
        }
        {
            QFile output_file(destination);
            if (!output_file.open(QIODevice::WriteOnly | QIODevice::Truncate) ||
                output_file.write(function_list) != function_list.size() || !output_file.flush()) {
                QTextStream(stderr)
                    << "Cannot write QtTest function list to " << destination << ".\n";
                return 2;
            }
        }
        return 0;
    }

    QList<int> selected = selectedClasses(arguments, tests);
    if (selected.isEmpty()) {
        if (hasUnknownFilter(arguments, tests)) {
            selected.append(0);
        } else {
            for (qsizetype index = 0; index < static_cast<qsizetype>(tests.size()); ++index)
                selected.append(static_cast<int>(index));
        }
    }

    int result = 0;
    if (arguments.contains(QStringLiteral("-datatags")) && selected.size() > 1) {
        QByteArray data_tags;
        QList<QPair<QString, bool>> output_targets;
        for (qsizetype argument_index = 1; argument_index + 1 < arguments.size();
             ++argument_index) {
            if (arguments.at(argument_index) == QStringLiteral("-o")) {
                const QString& output_argument = arguments.at(++argument_index);
                output_targets.append(
                    {outputPath(output_argument), outputFormatIsExplicit(output_argument)});
            }
        }
        const bool has_explicit_output =
            std::any_of(output_targets.cbegin(), output_targets.cend(),
                        [](const QPair<QString, bool>& target) { return target.second; });
        QStringList destinations;
        if (has_explicit_output) {
            for (const auto& [target, explicit_format] : output_targets) {
                if (explicit_format)
                    destinations.append(target);
            }
        } else if (!output_targets.isEmpty()) {
            destinations.append(output_targets.last().first);
        }
        for (const int index : selected) {
            QProcess process;
            const QStringList filtered_arguments =
                argumentsForClass(arguments, index, tests, selected, {}, {});
            QStringList child_arguments{filtered_arguments.cbegin() + 1, filtered_arguments.cend()};
            for (qsizetype argument_index = 0; argument_index < child_arguments.size();) {
                if (child_arguments.at(argument_index) == QStringLiteral("-o")) {
                    child_arguments.removeAt(argument_index);
                    if (argument_index < child_arguments.size())
                        child_arguments.removeAt(argument_index);
                    continue;
                }
                if (!legacyOutputFormat(child_arguments.at(argument_index)).isEmpty()) {
                    child_arguments.removeAt(argument_index);
                    continue;
                }
                ++argument_index;
            }
            child_arguments.append(QStringLiteral("--caret-test-class"));
            child_arguments.append(QString::number(index));
            process.start(QCoreApplication::applicationFilePath(), child_arguments);
            if (!process.waitForStarted() || !process.waitForFinished(-1)) {
                QTextStream(stderr)
                    << "Cannot collect QtTest data tags: " << process.errorString() << ".\n";
                const QByteArray error = process.readAllStandardError();
                fwrite(error.constData(), sizeof(char), static_cast<std::size_t>(error.size()),
                       stderr);
                return 2;
            }
            data_tags.append(process.readAllStandardOutput());
            const QByteArray error = process.readAllStandardError();
            if (!error.isEmpty())
                fwrite(error.constData(), sizeof(char), static_cast<std::size_t>(error.size()),
                       stderr);
            if (process.exitStatus() != QProcess::NormalExit || process.exitCode() != 0)
                result |= process.exitCode() == 0 ? 1 : process.exitCode();
        }
        if (destinations.isEmpty())
            destinations.append(QStringLiteral("-"));
        for (const QString& destination : destinations) {
            if (destination == QStringLiteral("-")) {
                QTextStream(stdout) << data_tags;
            } else {
                QFile output(destination);
                if (!output.open(QIODevice::WriteOnly | QIODevice::Truncate) ||
                    output.write(data_tags) != data_tags.size() || !output.flush()) {
                    QTextStream(stderr)
                        << "Cannot write QtTest data tags to " << destination << ".\n";
                    return 2;
                }
            }
        }
        return result;
    }

    struct OutputLogger {
        QString target;
        QString format;
        bool explicit_format;
        bool enabled;
    };
    QList<OutputLogger> loggers;
    QList<qsizetype> paired_legacy_positions;
    QList<QPair<qsizetype, QString>> legacy_formats;
    for (qsizetype index = 1; index < arguments.size(); ++index) {
        const QString& argument = arguments.at(index);
        if (argument == QStringLiteral("-o") && index + 1 < arguments.size()) {
            const QString& output_argument = arguments.at(++index);
            loggers.append({outputPath(output_argument), outputFormat(output_argument),
                            outputFormatIsExplicit(output_argument), true});
        } else {
            const QString format = legacyOutputFormat(argument);
            if (!format.isEmpty())
                legacy_formats.append({index, format});
        }
    }
    QList<qsizetype> default_logger_indices;
    bool has_explicit_logger = false;
    for (qsizetype index = 0; index < loggers.size(); ++index) {
        if (!loggers.at(index).explicit_format)
            default_logger_indices.append(index);
        else
            has_explicit_logger = true;
    }
    if (!loggers.isEmpty()) {
        if (has_explicit_logger) {
            for (const qsizetype index : default_logger_indices)
                loggers[index].enabled = false;
        } else {
            for (qsizetype index = 0; index + 1 < default_logger_indices.size(); ++index)
                loggers[default_logger_indices.at(index)].enabled = false;
        }
        if (!legacy_formats.isEmpty()) {
            for (qsizetype index = 0; index < legacy_formats.size(); ++index)
                paired_legacy_positions.append(legacy_formats.at(index).first);
            if (!has_explicit_logger && !default_logger_indices.isEmpty())
                loggers[default_logger_indices.last()].format = legacy_formats.last().second;
        }
    } else if (!legacy_formats.isEmpty()) {
        for (qsizetype index = 0; index + 1 < legacy_formats.size(); ++index)
            paired_legacy_positions.append(legacy_formats.at(index).first);
        loggers.append({QStringLiteral("-"), legacy_formats.last().second, true, true});
    }
    const bool has_active_logger =
        std::any_of(loggers.cbegin(), loggers.cend(),
                    [](const OutputLogger& logger) { return logger.enabled; });
    const bool collect_output = has_active_logger && selected.size() > 1;
    QTemporaryDir output_directory;
    if (collect_output && !output_directory.isValid()) {
        QTextStream(stderr) << "Cannot create temporary directory for QtTest output.\n";
        return 2;
    }
    QList<QList<QByteArray>> reports(loggers.size());
    for (const int index : selected) {
        QStringList redirected_outputs;
        if (collect_output) {
            for (qsizetype logger_index = 0; logger_index < loggers.size(); ++logger_index) {
                if (!loggers.at(logger_index).enabled) {
                    redirected_outputs.append(QString());
                    continue;
                }
                QString redirected = output_directory.filePath(
                    QStringLiteral("class-%1-logger-%2.log").arg(index).arg(logger_index));
                redirected.append(QLatin1Char(','));
                redirected.append(loggers.at(logger_index).format);
                redirected_outputs.append(redirected);
            }
        }
        const QStringList class_arguments = argumentsForClass(
            arguments, index, tests, selected, redirected_outputs, paired_legacy_positions);
        result |= QTest::qExec(tests[static_cast<std::size_t>(index)], class_arguments);
        if (collect_output) {
            for (qsizetype logger_index = 0; logger_index < loggers.size(); ++logger_index) {
                if (!loggers.at(logger_index).enabled)
                    continue;
                QFile report(output_directory.filePath(
                    QStringLiteral("class-%1-logger-%2.log").arg(index).arg(logger_index)));
                if (!report.open(QIODevice::ReadOnly)) {
                    QTextStream(stderr) << "Cannot read QtTest output for class " << index << ".\n";
                    return 2;
                }
                reports[logger_index].append(report.readAll());
            }
        }
    }
    if (collect_output) {
        for (qsizetype logger_index = 0; logger_index < loggers.size(); ++logger_index) {
            if (!loggers.at(logger_index).enabled)
                continue;
            const QByteArray merged =
                mergeReports(reports.at(logger_index), loggers.at(logger_index).format);
            if (loggers.at(logger_index).target == QStringLiteral("-")) {
                QTextStream(stdout) << merged;
                continue;
            }
            QFile output(loggers.at(logger_index).target);
            if (!output.open(QIODevice::WriteOnly | QIODevice::Truncate) ||
                output.write(merged) != merged.size() || !output.flush()) {
                QTextStream(stderr) << "Cannot write merged QtTest output to "
                                    << loggers.at(logger_index).target << ".\n";
                return 2;
            }
        }
    }
    return result;
}
