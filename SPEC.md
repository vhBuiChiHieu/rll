# Spec: rls - Rust ls

## 1. Mục tiêu

Xây dựng CLI app native tên `rls` (`rust ls`) để liệt kê tất cả file và thư mục ở vị trí hiện tại, kèm kích thước từng mục.

Ưu tiên theo thứ tự:

1. Hiệu suất cao nhất có thể.
2. Dùng ít RAM.
3. Binary nhẹ, ít dependency.
4. Output rõ, dễ parse về sau.

## 2. Phạm vi MVP

MVP chỉ xử lý current directory, không quét đệ quy.

Khi chạy command trong thư mục nào, app đọc nội dung trực tiếp của thư mục đó và in ra danh sách:

- Loại mục: file hoặc directory.
- Tên mục.
- Kích thước.

Ví dụ:

```text
TYPE  SIZE       NAME
DIR   4.0 KiB    .claude
DIR   4.0 KiB    .firecrawl
FILE  1.2 KiB    Coder Night Calm.yaml
```

## 3. Ngoài phạm vi MVP

Các tính năng sau chưa làm ở bản đầu:

- Quét đệ quy toàn bộ cây thư mục.
- Filter theo extension, glob, hidden file.
- Sort nâng cao.
- JSON output.
- Ignore rules như `.gitignore`.
- Progress bar.
- Watch mode.
- UI dạng TUI.

Các mục này có thể thêm sau khi MVP ổn và benchmark xong.

## 4. Yêu cầu chức năng

### 4.1 Current directory

- App mặc định quét thư mục hiện tại: `.`.
- Không yêu cầu truyền path ở MVP.
- Nếu sau này cần truyền path, thêm flag riêng, không làm phức tạp MVP.

### 4.2 Loại mục

App phải phân biệt tối thiểu:

- `FILE`: file thường.
- `DIR`: thư mục.
- `OTHER`: symlink, device, pipe, hoặc loại đặc biệt khác nếu hệ điều hành trả về.

Phân loại dùng `DirEntry::file_type()`:

- `is_file()` → `FILE`.
- `is_dir()` → `DIR`.
- Còn lại → `OTHER`.

### 4.3 Kích thước

- File: dùng kích thước metadata từ filesystem.
- Directory: dùng kích thước entry directory do OS trả về, không tính tổng kích thước con trong MVP.
- Size lấy bằng `DirEntry::metadata()` nếu được; nếu lỗi metadata, in size là `?`.
- Không đọc nội dung file để tính size.
- Không duyệt con của directory để tính recursive size ở MVP.

Lý do: đọc metadata rẻ hơn rất nhiều so với mở/đọc file hoặc duyệt cây con.

### 4.4 Output

Output mặc định dạng bảng text:

```text
TYPE  SIZE       NAME
FILE  120 B      a.txt
DIR   4.0 KiB    src
```

Yêu cầu:

- Mỗi entry một dòng.
- Không in thêm log thừa.
- Không dùng màu mặc định để tránh overhead và giúp output dễ pipe.
- Tên file giữ nguyên, không normalize không cần thiết.
- Tên file dùng `to_string_lossy()` để tránh panic với tên không UTF-8.

### 4.5 Thứ tự output

MVP ưu tiên tốc độ hơn sort.

- Mặc định: giữ thứ tự filesystem trả về.
- Không sort mặc định vì sort cần giữ toàn bộ entry trong memory.
- Nếu sau này thêm sort, phải là optional flag.

## 5. Yêu cầu phi chức năng

## 5.1 Hiệu suất

Hiệu suất là ưu tiên cao nhất.

Nguyên tắc:

- Dùng API filesystem trực tiếp, tối thiểu allocation.
- Stream output từng entry, không gom toàn bộ danh sách vào RAM.
- Không sort mặc định.
- Không đọc nội dung file.
- Không tính recursive directory size mặc định.
- Không dùng runtime nặng.
- Không dùng dependency nếu stdlib đáp ứng đủ.

Mục tiêu MVP:

- Duyệt thư mục theo streaming iterator.
- Mỗi entry chỉ gọi metadata cần thiết.
- In output qua buffered writer.

### 5.2 RAM

RAM phải tăng rất ít theo số lượng entry.

Yêu cầu:

- Không lưu `Vec<Entry>` cho toàn bộ thư mục.
- Xử lý entry nào in entry đó.
- Dùng `BufWriter` cho stdout.
- Tránh clone string/path không cần thiết.

Mục tiêu: memory gần O(1) theo số lượng entry, trừ buffer stdout và biến tạm cho entry hiện tại.

### 5.3 Binary nhẹ

Yêu cầu:

- Ưu tiên Rust stdlib.
- Không thêm CLI framework ở MVP nếu chưa cần.
- Không thêm logging framework.
- Không thêm format/table crate.

Có thể dùng argument parsing thủ công ở MVP vì chưa có flag.

### 5.4 Cross-platform

MVP nên chạy trên:

- Windows.
- Linux.
- macOS.

Không dùng API OS-specific nếu chưa cần.

## 6. Công nghệ đề xuất

Ngôn ngữ: Rust.

Lý do:

- Native binary, không cần runtime ngoài.
- Hiệu suất filesystem tốt.
- Quản lý memory chặt.
- Phù hợp CLI nhẹ.
- Cargo build/release đơn giản.

Dependency MVP:

```toml
[dependencies]
```

Không dependency nếu có thể.

## 7. Thiết kế xử lý

Luồng chính:

1. Lấy current directory: `std::env::current_dir()` hoặc dùng trực tiếp `.`.
2. Gọi `std::fs::read_dir(".")`.
3. Tạo `BufWriter<std::io::StdoutLock>`.
4. In header.
5. Với từng `DirEntry`:
   - Lấy file type bằng `entry.file_type()`.
   - Lấy metadata bằng `entry.metadata()` để lấy size.
   - Format type.
   - Format size.
   - In một dòng.
6. Flush stdout.

Không làm:

- Không sort.
- Không canonicalize path.
- Không convert path thành absolute path.
- Không đọc file content.
- Không scan thư mục con.

## 8. Format size

MVP nên dùng human-readable size để dễ đọc:

- `< 1024`: `B`.
- `< 1024^2`: `KiB`.
- `< 1024^3`: `MiB`.
- Lớn hơn: `GiB`.

Ví dụ:

```text
0 B
912 B
1.5 KiB
20.3 MiB
```

Lưu ý hiệu suất:

- Hàm format size phải đơn giản.
- Không dùng crate ngoài.
- Không allocate nhiều string nếu tránh được.

## 9. Error handling

MVP xử lý lỗi tối thiểu nhưng rõ:

- Nếu không đọc được current directory: in lỗi ra stderr, exit code `1`.
- Nếu một entry lỗi khi đọc: in dòng cảnh báo ngắn ra stderr, tiếp tục entry khác.
- Nếu không lấy được metadata cho entry: in size là `?`, vẫn in name nếu có.

Không panic với lỗi filesystem thường gặp.

## 10. CLI behavior

Command dự kiến:

```bash
rls
```

Output stdout chứa danh sách file/folder.

stderr chỉ dùng cho lỗi/cảnh báo.

Exit code:

| Code | Ý nghĩa |
|---|---|
| 0 | Thành công hoặc stdout gặp `BrokenPipe` khi pipe sang command khác |
| 1 | Không đọc được thư mục hiện tại hoặc lỗi nghiêm trọng |

## 11. Benchmark cần có sau MVP

Sau khi MVP chạy được, benchmark trên:

1. Thư mục nhỏ: dưới 100 entry.
2. Thư mục vừa: vài nghìn entry.
3. Thư mục lớn: hàng trăm nghìn entry nếu có dữ liệu test.

Chỉ số cần đo:

- Wall time.
- Peak RSS/RAM.
- Binary size release.
- Số syscall metadata nếu cần tối ưu sâu.

Command gợi ý:

```bash
cargo build --release
hyperfine './target/release/rls'
```

Nếu không có `hyperfine`, dùng `time` trước, chưa cần thêm tool.

## 12. Hướng mở rộng sau MVP

Các tính năng sau chỉ thêm khi cần, phải giữ nguyên nguyên tắc performance-first:

| Tính năng | Ghi chú hiệu suất |
|---|---|
| Recursive scan | dùng streaming traversal, tránh giữ toàn cây trong RAM |
| Parallel traversal | cân nhắc `jwalk` hoặc custom worker pool khi scan rất lớn |
| Ignore rules | cân nhắc crate `ignore`, nhưng chỉ khi cần `.gitignore` |
| JSON output | stream JSON lines trước, tránh build array lớn |
| Sorting | optional, cảnh báo tốn RAM với thư mục lớn |
| Summary | cộng dồn counters streaming, không lưu entries |
| Max depth | giảm I/O cho scan lớn |
| Extension filter | filter sớm trước metadata nếu có thể |

## 13. Tiêu chí hoàn thành MVP

MVP được coi là xong khi:

- Chạy command trong current directory và in ra file/folder trực tiếp.
- Mỗi dòng có type, size, name.
- Không quét đệ quy.
- Không sort mặc định.
- Không dependency ngoài nếu không cần.
- Dùng buffered stdout.
- Không đọc nội dung file.
- Build release thành công.
- Chạy được trên Windows hiện tại.

## 14. Nguyên tắc quan trọng cần giữ

Khi phát triển tiếp, mọi quyết định phải ưu tiên:

1. Giảm I/O không cần thiết.
2. Giảm allocation.
3. Stream thay vì collect.
4. Không sort mặc định.
5. Không đọc file content nếu metadata đủ.
6. Không thêm dependency khi stdlib đủ.
7. Tối ưu output vì terminal có thể là bottleneck lớn hơn scan.
