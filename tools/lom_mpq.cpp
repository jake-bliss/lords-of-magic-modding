#include <StormLib.h>

#include <algorithm>
#include <cctype>
#include <cstdint>
#include <filesystem>
#include <iomanip>
#include <iostream>
#include <sstream>
#include <stdexcept>
#include <string>
#include <unordered_set>
#include <vector>

namespace fs = std::filesystem;

namespace {

struct Archive {
  HANDLE handle = nullptr;

  explicit Archive(const fs::path &path) {
    if (!SFileOpenArchive(path.c_str(), 0, MPQ_OPEN_READ_ONLY, &handle)) {
      throw std::runtime_error("could not open MPQ: " + path.string());
    }
  }

  ~Archive() {
    if (handle != nullptr) {
      SFileCloseArchive(handle);
    }
  }

  Archive(const Archive &) = delete;
  Archive &operator=(const Archive &) = delete;
};

struct Entry {
  std::string name;
  std::uint32_t size;
  std::uint32_t compressed_size;
  std::uint32_t flags;
  std::uint32_t locale;
};

void load_internal_listfile(HANDLE archive) {
  HANDLE listfile = nullptr;
  if (!SFileOpenFileEx(archive, "(listfile)", SFILE_OPEN_FROM_MPQ, &listfile)) {
    return;
  }

  const DWORD size = SFileGetFileSize(listfile, nullptr);
  if (size == SFILE_INVALID_SIZE) {
    SFileCloseFile(listfile);
    throw std::runtime_error("could not determine (listfile) size");
  }

  std::string contents(size, '\0');
  DWORD bytes_read = 0;
  const bool read_ok =
      SFileReadFile(listfile, contents.data(), size, &bytes_read, nullptr);
  SFileCloseFile(listfile);
  if (!read_ok || bytes_read != size) {
    throw std::runtime_error("could not read internal (listfile)");
  }

  std::vector<std::string> names;
  std::istringstream lines(contents);
  for (std::string line; std::getline(lines, line);) {
    if (!line.empty() && line.back() == '\r') {
      line.pop_back();
    }
    if (!line.empty()) {
      names.push_back(std::move(line));
    }
  }

  std::vector<const char *> pointers;
  pointers.reserve(names.size());
  for (const auto &name : names) {
    pointers.push_back(name.c_str());
  }
  if (!pointers.empty()) {
    const DWORD result = SFileAddListFileEntries(
        archive, pointers.data(), static_cast<DWORD>(pointers.size()));
    if (result != ERROR_SUCCESS) {
      throw std::runtime_error("could not load internal (listfile) entries");
    }
  }
}

std::vector<Entry> list_entries(HANDLE archive) {
  load_internal_listfile(archive);
  SFILE_FIND_DATA data{};
  HANDLE search = SFileFindFirstFile(archive, "*", &data, nullptr);
  if (search == nullptr) {
    throw std::runtime_error(
        "could not enumerate MPQ; it may not contain a usable (listfile)");
  }

  std::vector<Entry> entries;
  do {
    entries.push_back({data.cFileName, data.dwFileSize, data.dwCompSize,
                       data.dwFileFlags, data.lcLocale});
  } while (SFileFindNextFile(search, &data));
  SFileFindClose(search);

  std::sort(entries.begin(), entries.end(), [](const Entry &a, const Entry &b) {
    return a.name < b.name;
  });
  return entries;
}

fs::path safe_relative_path(std::string name) {
  std::replace(name.begin(), name.end(), '\\', '/');
  fs::path relative(name);
  if (relative.is_absolute()) {
    throw std::runtime_error("refusing absolute archive path: " + name);
  }
  for (const auto &part : relative) {
    if (part == "..") {
      throw std::runtime_error("refusing archive path traversal: " + name);
    }
  }
  return relative;
}

void print_usage(const char *program) {
  std::cerr << "Usage:\n"
            << "  " << program << " list ARCHIVE.mpq\n"
            << "  " << program << " extract ARCHIVE.mpq OUTPUT_DIR\n";
}

int list_archive(const fs::path &archive_path) {
  Archive archive(archive_path);
  const auto entries = list_entries(archive.handle);

  std::cout << "path\tsize\tcompressed_size\tflags\tlocale\n";
  for (const auto &entry : entries) {
    std::cout << entry.name << '\t' << entry.size << '\t'
              << entry.compressed_size << "\t0x" << std::hex << std::setw(8)
              << std::setfill('0') << entry.flags << std::dec << '\t'
              << entry.locale << '\n';
  }
  return 0;
}

int extract_archive(const fs::path &archive_path, const fs::path &output_dir) {
  Archive archive(archive_path);
  const auto entries = list_entries(archive.handle);
  if (fs::exists(output_dir)) {
    if (!fs::is_directory(output_dir) || !fs::is_empty(output_dir)) {
      throw std::runtime_error("output directory must be new or empty: " +
                               output_dir.string());
    }
  } else {
    fs::create_directories(output_dir);
  }

  std::size_t extracted = 0;
  std::size_t duplicate_paths = 0;
  std::unordered_set<std::string> destination_keys;
  for (const auto &entry : entries) {
    const fs::path relative = safe_relative_path(entry.name);
    std::string destination_key = relative.generic_string();
    std::transform(destination_key.begin(), destination_key.end(),
                   destination_key.begin(), [](unsigned char character) {
                     return static_cast<char>(std::tolower(character));
                   });
    if (!destination_keys.insert(destination_key).second) {
      ++duplicate_paths;
      std::cerr << "lom-mpq: duplicate archive pathname overwrites prior entry: "
                << entry.name << '\n';
    }

    const fs::path destination = output_dir / relative;
    fs::create_directories(destination.parent_path());
    if (!SFileExtractFile(archive.handle, entry.name.c_str(), destination.c_str(),
                          SFILE_OPEN_FROM_MPQ)) {
      throw std::runtime_error("could not extract: " + entry.name);
    }
    ++extracted;
  }

  std::cout << "Extracted " << extracted << " files from "
            << archive_path.filename().string() << " to " << output_dir.string()
            << " (" << (extracted - duplicate_paths) << " unique paths)\n";
  return 0;
}

}  // namespace

int main(int argc, char **argv) {
  try {
    if (argc == 3 && std::string(argv[1]) == "list") {
      return list_archive(argv[2]);
    }
    if (argc == 4 && std::string(argv[1]) == "extract") {
      return extract_archive(argv[2], argv[3]);
    }
    print_usage(argv[0]);
    return 2;
  } catch (const std::exception &error) {
    std::cerr << "lom-mpq: " << error.what() << '\n';
    return 1;
  }
}
