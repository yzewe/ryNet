#pragma once

#include "model.h"

#include <QRect>
#include <QString>

namespace Store {
QString userDir();
void setExeDir(const QString &dir);
QString portableDir();
QString dir();
QString path(const QString &name);

QList<Sub> subs();
Settings settings();
void save(const QList<Sub> &subs, const Settings &settings);

std::optional<QString> body(const QString &id);
void saveBody(const QString &id, const QString &text);
void removeBody(const QString &id);

struct Window {
    int w = 0, h = 0;
    bool max = false;
};
std::optional<Window> window();
void saveWindow(const Window &w);

bool write(const QString &file, const QByteArray &data);
}
