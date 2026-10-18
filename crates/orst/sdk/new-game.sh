#!/bin/sh
# Makes a new game from a template of this SDK:
#     ./new-game.sh MyGame          a C# game in ./MyGame
#     ./new-game.sh MyGame cpp      a C++ game (CMake)
set -e
name=$1
lang=${2:-cs}
dir=${3:-$PWD/$name}
case $name in
    [A-Za-z]*) ;;
    *) echo "usage: $0 Name [cs|cpp] [folder]" >&2; exit 1 ;;
esac
if printf '%s' "$name" | grep -q '[^A-Za-z0-9_]'; then
    echo "The name is a Latin letter then letters, digits or _ (it becomes the program name): $name" >&2
    exit 1
fi
case $lang in
    cpp) template=cpp ;;
    cs) template=csharp ;;
    *) echo "language: cs or cpp" >&2; exit 1 ;;
esac
if [ -e "$dir" ]; then echo "$dir already exists" >&2; exit 1; fi

sdk=$(cd "$(dirname "$0")" && pwd)
version=$(cat "$sdk/VERSION")
cp -R "$sdk/templates/$template" "$dir"
find "$dir" -type f | while read -r f; do
    sed -i.bak -e "s|__NAME__|$name|g" -e "s|__SDK__|$sdk|g" -e "s|__VERSION__|$version|g" "$f"
    rm -f "$f.bak"
    case $f in *__NAME__*) mv "$f" "$(dirname "$f")/$(basename "$f" | sed "s|__NAME__|$name|g")" ;; esac
done

echo "Done: $dir"
if [ "$lang" = cpp ]; then
    echo "cd $name && cmake -S . -B build && cmake --build build && ./build/$name"
else
    echo "cd $name && dotnet run"
fi
