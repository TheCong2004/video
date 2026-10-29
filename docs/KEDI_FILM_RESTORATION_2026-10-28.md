# Khôi phục Làm phim từ ý tưởng

Khôi phục chức năng Làm phim AI thành mục riêng bên cạnh Kedi Factory. Các dự án phim cũ vẫn dùng thư mục `.runtime/films` hiện có.

## Mã đã khôi phục

- `src/crates/concat/ui/start.slint`: mục Làm phim AI độc lập, chuyển tới `StudioRoute.film`; trạng thái chọn của Kedi Factory chỉ áp dụng cho route Factory. Các mục menu là phần tử ngang hàng, tránh lỗi đặt lồng gây chồng chữ.
- `src/crates/concat/ui/workspace/studio-pane.slint`: route film mở `StudioFilmPage`, tiêu đề Làm phim từ ý tưởng.
- `src/crates/concat/src/lib.rs`: đăng ký lại `studio_film::bind` để các nút tạo phim, lưu dự án, chọn phim và mở đầu ra nối với handler hiện có.
- `src/crates/concat/src/studio_film.rs`: giữ luồng dự án và xử lý phim, bỏ việc tự tạo thư mục mẫu hoặc hàng dự án hoàn thành giả khi danh sách trống.
- `src/crates/concat/ui/workspace/studio-film.slint`: danh sách mặc định rỗng, dữ liệu lấy từ dự án đã lưu; phần tiêu đề và nút thao tác xuống dòng để tránh tràn.

## Bảo toàn dự án

Đã ghi nhận SHA256 các `project.json` trước khi mở tính năng khôi phục tại `.runtime/kedi-restore-film-projects-before.json`. Không di chuyển hoặc xóa dự án phim. Có một tệp dự án đã lưu trong lần kiểm tra ban đầu.

## Kiểm tra

- 7 kiểm tra design/resource hiện có: pass; log `.runtime/kedi-restore-film-layout-tests.log`.
- Build native thành công, exit 0; log `.runtime/kedi-restore-film-build-final.log`. Lần đầu bị khóa EXE bởi cửa sổ cũ; đóng cửa sổ trống và link lại thành công.
- `--version` và `--smoke`: exit 0, phiên bản 0.2.7. Smoke trong sandbox báo không ghi được log ở AppData; chạy GUI bằng launcher với profile thật thành công.
- Kiểm tra trực tiếp EXE mới: menu có Kedi Factory / Làm phim AI / Quy trình nâng cao riêng; chọn Làm phim AI mở đúng trang Làm phim từ ý tưởng; chọn dự án Bát Tràng đã lưu tải lại tên và ý tưởng qua handler. Không chạy tạo phim mới trong lần kiểm tra này.
- SHA256 `project.json` sau kiểm tra khớp bản ghi trước, không thay đổi dữ liệu phim.
- EXE mới đã cập nhật ở `src/target/debug/concat.exe` và `dist/kedi-test/concat.exe`: SHA256 `1540E7CC88C1928942A2BAD41EA75E46907D5249022DCA4DF5D85646DC8BAB52`, 289497088 bytes. Bản build cũ ở thư mục test đã được thay thế.
- Mở bằng `run-concat.bat` hoặc `scripts/windows-studio.ps1 -RunBuilt`. Cửa sổ đã mở để người dùng kiểm tra.

Factory vẫn dùng ảnh/video thật và hỗ trợ nhập file âm thanh. Việc khôi phục trang Làm phim từ ý tưởng không thay đổi bộ tải hoặc renderer của Factory.
