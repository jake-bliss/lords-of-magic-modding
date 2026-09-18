#include <CommonCrypto/CommonDigest.h>
#include <StormLib.h>

#include <algorithm>
#include <array>
#include <cctype>
#include <cstdint>
#include <cstdio>
#include <filesystem>
#include <fstream>
#include <iomanip>
#include <iostream>
#include <iterator>
#include <map>
#include <sstream>
#include <stdexcept>
#include <string>
#include <unordered_set>
#include <utility>
#include <vector>

namespace fs = std::filesystem;

namespace {

struct Archive {
  HANDLE handle = nullptr;

  Archive() = default;

  explicit Archive(const fs::path &path, DWORD flags = MPQ_OPEN_READ_ONLY) {
    if (!SFileOpenArchive(path.c_str(), 0, flags, &handle)) {
      throw std::runtime_error("could not open MPQ: " + path.string() +
                               " (StormLib error " +
                               std::to_string(SErrGetLastError()) + ")");
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
  std::uint32_t block_index;
  std::uint32_t hash_index;
};

// Storage flags worth carrying from a source member onto its replacement. The
// high MPQ_FILE_EXISTS bit is deliberately excluded: SFileAddFileEx reads that
// same bit position as MPQ_FILE_REPLACEEXISTING.
constexpr std::uint32_t kPreservedStorageFlags =
    MPQ_FILE_IMPLODE | MPQ_FILE_COMPRESS | MPQ_FILE_ENCRYPTED |
    MPQ_FILE_FIX_KEY | MPQ_FILE_SINGLE_UNIT | MPQ_FILE_SECTOR_CRC;

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

// Load a catalogue of recovered names, so that members the archive itself
// cannot name are still enumerated and extracted under their real names.
//
// The names are **candidates**, not an addressing table. StormLib resolves each
// one against the archive in front of it, by hash, at the moment of use; a name
// the archive does not hold simply does not appear. Nothing is keyed on the
// `File%08u.xxx` pseudo-name, which is a block position and moves when an
// archive is rewritten.
void load_external_listfile(HANDLE archive, const fs::path &path) {
  std::ifstream input(path);
  if (!input) {
    throw std::runtime_error("could not read listfile: " + path.string());
  }
  std::vector<std::string> names;
  for (std::string line; std::getline(input, line);) {
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
  if (pointers.empty()) {
    return;
  }
  if (SFileAddListFileEntries(archive, pointers.data(),
                              static_cast<DWORD>(pointers.size())) !=
      ERROR_SUCCESS) {
    throw std::runtime_error("could not load listfile: " + path.string());
  }
}

std::vector<Entry> list_entries(HANDLE archive,
                                const fs::path &extra_listfile = fs::path()) {
  load_internal_listfile(archive);
  if (!extra_listfile.empty()) {
    load_external_listfile(archive, extra_listfile);
  }
  SFILE_FIND_DATA data{};
  HANDLE search = SFileFindFirstFile(archive, "*", &data, nullptr);
  if (search == nullptr) {
    throw std::runtime_error(
        "could not enumerate MPQ; it may not contain a usable (listfile)");
  }

  std::vector<Entry> entries;
  do {
    entries.push_back({data.cFileName, data.dwFileSize, data.dwCompSize,
                       data.dwFileFlags, data.lcLocale, data.dwBlockIndex,
                       data.dwHashIndex});
  } while (SFileFindNextFile(search, &data));
  SFileFindClose(search);

  std::sort(entries.begin(), entries.end(), [](const Entry &a, const Entry &b) {
    if (a.name != b.name) {
      return a.name < b.name;
    }
    return a.block_index < b.block_index;
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


// Read one member's bytes. Members are addressed by BLOCK INDEX, not by name.
// PIC5R3 holds two distinct entries under the single name `portrait\AIpotM.lbm`
// (docs/mpq-inventory.md), and opening that name resolves to only one of them,
// so a name-addressed manifest cannot see that one of the two went missing.
// StormLib's `File%08u.xxx` pseudo-name addresses the block table directly.
std::string read_member_by_index(HANDLE archive, const Entry &entry) {
  std::array<char, 32> pseudo_name{};
  std::snprintf(pseudo_name.data(), pseudo_name.size(), "File%08u.xxx",
                entry.block_index);

  HANDLE file = nullptr;
  if (!SFileOpenFileEx(archive, pseudo_name.data(), SFILE_OPEN_FROM_MPQ,
                       &file)) {
    throw std::runtime_error("could not open block " +
                             std::to_string(entry.block_index) + " (" +
                             entry.name + ")");
  }

  const DWORD size = SFileGetFileSize(file, nullptr);
  if (size == SFILE_INVALID_SIZE) {
    SFileCloseFile(file);
    throw std::runtime_error("could not size member: " + entry.name);
  }

  std::string contents(size, '\0');
  DWORD bytes_read = 0;
  // A zero-byte member is legal. StormLib 9.40 happens to return success for a
  // zero-length read (measured 2026-09-18 by removing this guard and watching
  // the zero-byte member test still pass), so this short-circuit is defensive
  // rather than load-bearing. It stays because that success is not documented.
  const bool read_ok =
      size == 0 ||
      SFileReadFile(file, contents.data(), size, &bytes_read, nullptr);
  SFileCloseFile(file);
  if (!read_ok || bytes_read != size) {
    throw std::runtime_error("could not read member: " + entry.name);
  }
  return contents;
}

std::string sha256_hex(const std::string &data) {
  std::array<unsigned char, CC_SHA256_DIGEST_LENGTH> digest{};
  CC_SHA256(data.data(), static_cast<CC_LONG>(data.size()), digest.data());
  std::ostringstream text;
  text << std::hex << std::setfill('0');
  for (const unsigned char byte : digest) {
    text << std::setw(2) << static_cast<unsigned>(byte);
  }
  return text.str();
}

std::string read_local_file(const fs::path &path) {
  std::ifstream input(path, std::ios::binary);
  if (!input) {
    throw std::runtime_error("could not read local file: " + path.string());
  }
  return std::string((std::istreambuf_iterator<char>(input)),
                     std::istreambuf_iterator<char>());
}

// `archive\name=local/path`, split at the FIRST `=`. Local paths do contain
// `=` in practice (a temporary directory is enough to produce one) while no
// member name in any of the five installed archives contains one -- measured
// 2026-09-18 across all 3,106 gs/pic entries. An archive name containing `=` is
// therefore not addressable and is a known limit, not an oversight.
std::pair<std::string, fs::path> parse_assignment(const std::string &argument) {
  const auto separator = argument.find('=');
  if (separator == std::string::npos || separator == 0 ||
      separator + 1 == argument.size()) {
    throw std::runtime_error("expected ARCHIVE_NAME=LOCAL_PATH, got: " +
                             argument);
  }
  return {argument.substr(0, separator), fs::path(argument.substr(separator + 1))};
}

int manifest_archive(const fs::path &archive_path,
                     const fs::path &extra_listfile) {
  Archive archive(archive_path);
  const auto entries = list_entries(archive.handle, extra_listfile);

  std::cout << "path\tblock_index\thash_index\tsize\tcompressed_size\tflags"
               "\tlocale\tsha256\n";
  for (const auto &entry : entries) {
    const std::string contents = read_member_by_index(archive.handle, entry);
    if (contents.size() != entry.size) {
      throw std::runtime_error("short read for member: " + entry.name);
    }
    std::cout << entry.name << '\t' << entry.block_index << '\t'
              << entry.hash_index << '\t' << entry.size << '\t'
              << entry.compressed_size << "\t0x" << std::hex << std::setw(8)
              << std::setfill('0') << entry.flags << std::dec
              << std::setfill(' ') << '\t' << entry.locale << '\t'
              << sha256_hex(contents) << '\n';
  }
  return 0;
}

// Probe an archive for a candidate NAME without consulting its own
// `(listfile)`. SFileOpenFileEx hashes the name and looks it up in the hash
// table, so a successful open is a property of the archive's own hash table and
// is independent of whatever catalogue supplied the name. The block index and
// hash index come back from StormLib, which is what lets a caller join the hit
// onto a block-index-addressed manifest and demand that the digests agree.
//
// The internal listfile is deliberately NOT loaded here. Loading it would let a
// name that the target already knows resolve through StormLib's name cache
// rather than through the hash table, and the point of this verb is that the
// lookup mechanism is the same for a name the archive knows and a name it does
// not.
int probe_names(const fs::path &archive_path, const fs::path &names_path) {
  Archive archive(archive_path);

  // `-` reads the candidate list from standard input. A search wide enough to
  // make a bounded negative worth anything is wide enough that materialising it
  // as a file is the expensive part: 1.9 billion names is 31 GB on disk and
  // nothing on a pipe.
  std::ifstream file;
  if (names_path != "-") {
    file.open(names_path);
    if (!file) {
      throw std::runtime_error("could not read candidate name list: " +
                               names_path.string());
    }
  }
  std::istream &input = names_path == "-" ? std::cin : file;

  std::cout << "name\tstatus\tblock_index\thash_index\tsize\tsha256\n";
  for (std::string line; std::getline(input, line);) {
    if (!line.empty() && line.back() == '\r') {
      line.pop_back();
    }
    if (line.empty()) {
      continue;
    }

    HANDLE file = nullptr;
    if (!SFileOpenFileEx(archive.handle, line.c_str(), SFILE_OPEN_FROM_MPQ,
                         &file)) {
      std::cout << line << "\tabsent\t\t\t\t\n";
      continue;
    }

    DWORD block_index = 0;
    DWORD hash_index = 0;
    const bool have_block =
        SFileGetFileInfo(file, SFileInfoFileIndex, &block_index,
                         sizeof(block_index), nullptr);
    const bool have_hash =
        SFileGetFileInfo(file, SFileInfoHashIndex, &hash_index,
                         sizeof(hash_index), nullptr);

    const DWORD size = SFileGetFileSize(file, nullptr);
    if (size == SFILE_INVALID_SIZE) {
      SFileCloseFile(file);
      throw std::runtime_error("could not size member opened by name: " + line);
    }
    std::string contents(size, '\0');
    DWORD bytes_read = 0;
    const bool read_ok =
        size == 0 ||
        SFileReadFile(file, contents.data(), size, &bytes_read, nullptr);
    SFileCloseFile(file);
    if (!read_ok || bytes_read != size) {
      throw std::runtime_error("could not read member opened by name: " + line);
    }
    if (!have_block || !have_hash) {
      throw std::runtime_error("could not locate member opened by name: " +
                               line);
    }

    std::cout << line << "\tpresent\t" << block_index << '\t' << hash_index
              << '\t' << size << '\t' << sha256_hex(contents) << '\n';
  }
  return 0;
}

// Add one member, reporting what StormLib actually did rather than assuming.
void add_member(HANDLE archive, const std::string &archived_name,
                const fs::path &local_path, std::uint32_t storage_flags) {
  const std::uint32_t flags = storage_flags | MPQ_FILE_REPLACEEXISTING;
  // MPQ_FILE_COMPRESS needs a method; MPQ_FILE_IMPLODE ignores the argument.
  // PKWARE DCL is the method the 1997 engine is known to read, and is what the
  // whole of `gs.mpq` uses under MPQ_FILE_IMPLODE.
  const std::uint32_t compression =
      (storage_flags & MPQ_FILE_COMPRESS) != 0 ? MPQ_COMPRESSION_PKWARE : 0;
  if (!SFileAddFileEx(archive, local_path.c_str(), archived_name.c_str(), flags,
                      compression, compression)) {
    throw std::runtime_error("could not add member " + archived_name +
                             " from " + local_path.string() +
                             " (StormLib error " +
                             std::to_string(SErrGetLastError()) + ")");
  }
}

// `extra_listfile` supplies names the source archive does not carry itself.
// Without it this verb cannot address `pic.mpq`, `imp.mpq`, `sndfx.mpq` or
// `special.mpq` at all: none of the four has a `(listfile)`, so every member
// lists under the `File%08u.xxx` pseudo-name, and **Observed 2026-09-18** both
// ways round that fail. Naming a real member is refused below because the flags
// map has no such key; naming the pseudo-name reaches SFileAddFileEx, which
// rejects it with StormLib error 22 -- the pseudo-name is a read-side
// convenience that resolves by block position, and nothing ever hashes it into
// the hash table. A recovered name does hash to the member's existing
// hash-table entry, which is what makes the replacement a replacement rather
// than an addition.
int repack_archive(const fs::path &source_path, const fs::path &output_path,
                   const std::vector<std::string> &assignments, bool compact,
                   const fs::path &extra_listfile) {
  if (fs::exists(output_path)) {
    throw std::runtime_error("output archive already exists: " +
                             output_path.string());
  }
  if (assignments.empty()) {
    throw std::runtime_error("repack requires at least one --replace");
  }

  std::map<std::string, std::uint32_t> storage_flags;
  {
    Archive source(source_path);
    for (const auto &entry : list_entries(source.handle, extra_listfile)) {
      // Keep the FIRST entry for a duplicated name, matching the entry that
      // StormLib resolves that name to.
      storage_flags.emplace(entry.name, entry.flags & kPreservedStorageFlags);
    }
  }

  std::vector<std::pair<std::string, fs::path>> replacements;
  for (const auto &assignment : assignments) {
    auto parsed = parse_assignment(assignment);
    if (storage_flags.find(parsed.first) == storage_flags.end()) {
      throw std::runtime_error(
          "refusing to repack: source archive has no member named " +
          parsed.first);
    }
    if (!fs::is_regular_file(parsed.second)) {
      throw std::runtime_error("replacement is not a regular file: " +
                               parsed.second.string());
    }
    replacements.push_back(std::move(parsed));
  }
  // Applying replacements in a fixed order keeps repeated runs comparable.
  std::sort(replacements.begin(), replacements.end());

  // The archive is built beside the requested output and moved into place only
  // after it closes cleanly, so a failed repack never leaves a half-written
  // archive under the name a later step would install.
  const fs::path staging_path = output_path.string() + ".partial";
  fs::remove(staging_path);
  if (output_path.has_parent_path()) {
    fs::create_directories(output_path.parent_path());
  }
  fs::copy_file(source_path, staging_path);

  try {
    Archive staging(staging_path, 0);
    for (const auto &replacement : replacements) {
      add_member(staging.handle, replacement.first, replacement.second,
                 storage_flags.at(replacement.first));
    }
    // Compaction is off by default. SFileCompactArchive re-packs every member,
    // which it can only do if it knows every member's name -- the name is part
    // of the encryption key. Vanilla `gs.mpq` (372 unnamed entries) and every
    // `pic.mpq` (409 unnamed in PIC5R3) therefore fail it with
    // ERROR_UNKNOWN_FILE_NAMES (10007), measured 2026-09-18. Without it the
    // replaced member's old data stays in the file as dead space, which costs
    // size and nothing else.
    if (compact && !SFileCompactArchive(staging.handle, nullptr, false)) {
      throw std::runtime_error("could not compact archive (StormLib error " +
                               std::to_string(SErrGetLastError()) +
                               "; 10007 means the archive has members whose "
                               "names are unknown, which cannot be compacted)");
    }
  } catch (...) {
    fs::remove(staging_path);
    throw;
  }

  fs::rename(staging_path, output_path);
  std::cout << "Repacked " << source_path.filename().string() << " -> "
            << output_path.string() << " with " << replacements.size()
            << " replaced member(s)\n";
  for (const auto &replacement : replacements) {
    std::cout << "  " << replacement.first << " <- "
              << replacement.second.string() << '\n';
  }
  std::cout << "Output " << fs::file_size(output_path) << " bytes, sha256 "
            << sha256_hex(read_local_file(output_path)) << '\n';
  return 0;
}

// Archive creation exists so the repack pipeline can be tested on archives that
// look nothing like the shipped corpus -- an empty archive, a zero-byte member,
// names differing only in case. It is not a mod packaging command.
int create_archive(const fs::path &output_path,
                   const std::vector<std::string> &assignments,
                   std::uint32_t storage_flags, bool with_listfile) {
  if (fs::exists(output_path)) {
    throw std::runtime_error("output archive already exists: " +
                             output_path.string());
  }
  if (output_path.has_parent_path()) {
    fs::create_directories(output_path.parent_path());
  }

  std::vector<std::pair<std::string, fs::path>> members;
  for (const auto &assignment : assignments) {
    auto parsed = parse_assignment(assignment);
    if (!fs::is_regular_file(parsed.second)) {
      throw std::runtime_error("member source is not a regular file: " +
                               parsed.second.string());
    }
    members.push_back(std::move(parsed));
  }
  std::sort(members.begin(), members.end());

  HANDLE handle = nullptr;
  // StormLib rejects a zero maximum file count, so an empty archive still
  // reserves one slot.
  const DWORD capacity =
      static_cast<DWORD>(members.empty() ? 1 : members.size() + 1);
  // An archive with no `(listfile)` holds members it cannot name -- the shipped
  // corpus's normal condition, and the only fixture on which `--listfile` has
  // anything to do. MPQ_CREATE_LISTFILE on the simple SFileCreateArchive does
  // not achieve that: **Observed 2026-09-18**, omitting it still produced a
  // `(listfile)` member, because the simple entry point sets the internal-file
  // flags to their defaults regardless. Suppressing it needs
  // SFileCreateArchive2 with dwFileFlags1 = 0.
  if (with_listfile) {
    if (!SFileCreateArchive(output_path.c_str(),
                            MPQ_CREATE_LISTFILE | MPQ_CREATE_ARCHIVE_V1,
                            capacity, &handle)) {
      throw std::runtime_error("could not create archive: " +
                               output_path.string() + " (StormLib error " +
                               std::to_string(SErrGetLastError()) + ")");
    }
  } else {
    SFILE_CREATE_MPQ create_info{};
    create_info.cbSize = sizeof(create_info);
    create_info.dwMpqVersion = MPQ_FORMAT_VERSION_1;
    create_info.dwFileFlags1 = 0;
    create_info.dwFileFlags2 = 0;
    create_info.dwFileFlags3 = 0;
    // A zero sector size crashes StormLib 9.40 rather than defaulting; 4096 is
    // the value the simple entry point uses.
    create_info.dwSectorSize = 0x1000;
    create_info.dwMaxFileCount = capacity;
    if (!SFileCreateArchive2(output_path.c_str(), &create_info, &handle)) {
      throw std::runtime_error("could not create archive: " +
                               output_path.string() + " (StormLib error " +
                               std::to_string(SErrGetLastError()) + ")");
    }
  }
  Archive created{};
  created.handle = handle;

  for (const auto &member : members) {
    add_member(created.handle, member.first, member.second, storage_flags);
  }
  std::cout << "Created " << output_path.string() << " with " << members.size()
            << " member(s)\n";
  return 0;
}

void print_usage(const char *program) {
  std::cerr << "Usage:\n"
            << "  " << program << " list ARCHIVE.mpq [--listfile NAMES.txt]\n"
            << "  " << program
            << " extract ARCHIVE.mpq OUTPUT_DIR [--listfile NAMES.txt]\n"
            << "  " << program << " manifest ARCHIVE.mpq [--listfile NAMES.txt]\n"
            << "  " << program
            << " repack SOURCE.mpq OUTPUT.mpq [--compact] "
               "[--listfile NAMES.txt] --replace 'NAME=LOCAL' ...\n"
            << "  " << program
            << " create OUTPUT.mpq [--implode|--compress|--store] "
               "[--no-listfile] [--add 'NAME=LOCAL'] "
               "...\n"
            << "  " << program
            << " probe-names ARCHIVE.mpq NAMES.txt   (NAMES.txt may be `-` "
               "for stdin)\n";
}

int list_archive(const fs::path &archive_path,
                 const fs::path &extra_listfile) {
  Archive archive(archive_path);
  const auto entries = list_entries(archive.handle, extra_listfile);

  std::cout << "path\tsize\tcompressed_size\tflags\tlocale\n";
  for (const auto &entry : entries) {
    std::cout << entry.name << '\t' << entry.size << '\t'
              << entry.compressed_size << "\t0x" << std::hex << std::setw(8)
              << std::setfill('0') << entry.flags << std::dec << '\t'
              << entry.locale << '\n';
  }
  return 0;
}

int extract_archive(const fs::path &archive_path, const fs::path &output_dir,
                    const fs::path &extra_listfile) {
  Archive archive(archive_path);
  const auto entries = list_entries(archive.handle, extra_listfile);
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
    // `--listfile NAMES.txt` may trail list/extract/manifest. Recovered names
    // are supplied this way rather than being written into any archive: the
    // archive stays read-only and untouched, and each name is re-resolved
    // against it on every run.
    fs::path extra_listfile;
    int positional_argc = argc;
    if (argc >= 3 && std::string(argv[argc - 2]) == "--listfile") {
      extra_listfile = argv[argc - 1];
      positional_argc = argc - 2;
    }
    if (positional_argc == 3 && std::string(argv[1]) == "list") {
      return list_archive(argv[2], extra_listfile);
    }
    if (positional_argc == 4 && std::string(argv[1]) == "extract") {
      return extract_archive(argv[2], argv[3], extra_listfile);
    }
    if (positional_argc == 3 && std::string(argv[1]) == "manifest") {
      return manifest_archive(argv[2], extra_listfile);
    }
    if (argc == 4 && std::string(argv[1]) == "probe-names") {
      return probe_names(argv[2], argv[3]);
    }
    if (argc >= 5 && std::string(argv[1]) == "repack") {
      std::vector<std::string> assignments;
      bool compact = false;
      fs::path repack_listfile;
      for (int index = 4; index < argc; ++index) {
        const std::string option(argv[index]);
        if (option == "--compact") {
          compact = true;
        } else if (option == "--replace" && index + 1 < argc) {
          assignments.emplace_back(argv[++index]);
        } else if (option == "--listfile" && index + 1 < argc) {
          repack_listfile = argv[++index];
        } else {
          print_usage(argv[0]);
          return 2;
        }
      }
      return repack_archive(argv[2], argv[3], assignments, compact,
                            repack_listfile);
    }
    if (argc >= 3 && std::string(argv[1]) == "create") {
      std::vector<std::string> assignments;
      std::uint32_t storage_flags = MPQ_FILE_IMPLODE;
      bool with_listfile = true;
      for (int index = 3; index < argc; ++index) {
        const std::string option(argv[index]);
        if (option == "--no-listfile") {
          with_listfile = false;
        } else if (option == "--store") {
          storage_flags = 0;
        } else if (option == "--compress") {
          storage_flags = MPQ_FILE_COMPRESS;
        } else if (option == "--implode") {
          storage_flags = MPQ_FILE_IMPLODE;
        } else if (option == "--add" && index + 1 < argc) {
          assignments.emplace_back(argv[++index]);
        } else {
          print_usage(argv[0]);
          return 2;
        }
      }
      return create_archive(argv[2], assignments, storage_flags, with_listfile);
    }
    print_usage(argv[0]);
    return 2;
  } catch (const std::exception &error) {
    std::cerr << "lom-mpq: " << error.what() << '\n';
    return 1;
  }
}
