# damned.exe

Windows x64 için Rust + Windows API + Slint ile kompakt sistem monitörü.
Görsellerdeki sayılar veri kaynağı değildir; uygulamada rastgele/demo veri yoktur.

## Windows EXE indir

1. Bu klasörün **tüm kaynak dosyalarını, `Cargo.lock` ve `.github` dahil**, GitHub repona gönder.
2. **Actions → Windows x64** iş akışını aç. Her push'ta çalışır; **Run workflow** ile elle de başlatılabilir.
3. Başarılı çalışmanın **Artifacts → damned-windows-x64** paketini indir ve ZIP'in tamamını bir klasöre çıkar.
4. `damned.exe` dosyasını yanındaki DLL'lerle birlikte çalıştır. Yönetici olarak başlatmak gerekmez.

İş akışı `windows-2025`, Rust stable `x86_64-pc-windows-msvc` kullanır; biçim kontrolü ve birim testlerinden sonra tam olarak `cargo build --release --locked` çalıştırır. Başarılı derlemeden sonra `target/release/damned.exe`, eşleşen x64 Visual C++ CRT DLL'leri, bu belge, lisans ve SHA-256 özeti paketlenir. Paketleme eksikse iş akışı başarısız olur; eksik artifact yayımlamaz.

Slint arayüzü EXE içine derlenir. Yazılım çizicisi kullanılır; Qt, ayrı Slint DLL'i, font, resim veya `.slint` dosyası dağıtılmaz. Windows 10/11'in işletim sistemi DLL'leri pakete kopyalanmaz. Microsoft runtime dosyalarının hakları Microsoft'a aittir; projenin MIT lisansı yalnızca proje kaynaklarına uygulanır.

## Yerel derleme

Rust stable MSVC ve Visual Studio Build Tools'un **Desktop development with C++** iş yükü (MSVC x64/x86 araçları + Windows SDK) gerekir. PowerShell'de:

```powershell
rustup default stable-x86_64-pc-windows-msvc
cargo build --release --locked
cargo test --locked
.\target\release\damned.exe
```

EXE: `target/release/damned.exe`. Dağıtım klasörü üretmek için:

```powershell
.\scripts\package.ps1
```

Çıktı: `dist/damned-windows-x64/`. `Cargo.lock` kaynakla birlikte sürümlenmelidir; CI kilidi yeniden üretmez. Yerel `link.exe not found` hatası MSVC araçlarının eksik veya geliştirici ortamının ayarlanmamış olduğunu belirtir; Developer PowerShell for VS kullan.

## Kullanım

- Nokta seçici soldan sağa: özet + grafik, yalnızca liste, özet + liste, özet + grafik + liste. Fare, Tab/Enter/Space, `1–4` ve sol/sağ ok tuşlarıyla değiştirilebilir.
- Başlangıç yazı ölçeği `%90`. `+`/`−` veya numpad ekleme/çıkarma ile 10 puan değiştir; `0` ile sıfırla. Aralık `%70–130`.
- Sütun başlığı sıralar; tekrar tıklamak yönü değiştirir. Veri bulunmayan ağ/disk sütunları eşit kabul edilir; kararlı ad sırası korunur.
- Küçük ok aynı EXE yoluna ait işlem grubunu açar. Farklı kurulum yollarındaki aynı adlı EXE'ler ayrı gruplardır. Başlangıçta adı okunabilen fakat yolu okunamayan işlemler EXE adıyla gruplanır.
- Uzun adın veya metriğin üzerinde fareyi tutunca tam ad/açıklama görünür.
- Mod, ölçek ve sıralama `%LOCALAPPDATA%\damned\settings.json` içinde saklanır. Açık gruplar oturum boyunca modlar arasında korunur. Liste modeli yenilemede yeniden oluşturulmaz; kaydırma gereksiz yere sıfırlanmaz.
- Liste sanallaştırılır. Grafikler 60 saniyelik sınırlı geçmiş kullanır; görünmeyen grafikler çizilmez. Ölçüm UI dışında yaklaşık 1 Hz; küçültülmüş/gizli pencerede 5 saniyede bir yapılır. Örtülü fakat küçültülmemiş pencereler normal hızda örneklenir.

## Gerçek metriklerin kapsamı

| Metrik | Kaynak ve sınır |
| --- | --- |
| Toplam CPU | `GetSystemTimes`, iki örneğin farkı; ilk örnek `—`. |
| Toplam RAM | `GlobalMemoryStatusEx`, fiziksel kullanılan/toplam bellek. GB gösterimi 1024 tabanlıdır. |
| İşlem CPU | `GetProcessTimes`, zaman farkı / mantıksal işlemci sayısı. İşlem kimliği PID + oluşturulma zamanı ile izlenir. 64'ten fazla mantıksal işlemcili çok gruplu sistemler ayrıca doğrulanmalıdır. |
| İşlem RAM | `GetProcessMemoryInfo`, çalışma kümesi. Paylaşılan sayfalar birden çok işlemde sayılır; Görev Yöneticisi varsayılan özel çalışma kümesi sütunuyla birebir eşit değildir. Korumalı işlemlerde `—`. |
| GPU kullanımı | İngilizce PDH `GPU Engine(*)` sayaçları; aynı motorun işlem değerleri toplanır, tüm adaptörlerde en yoğun motor gösterilir. Uyumlu Windows/WDDM sürücüsü ve kullanılabilir iki örnek gerekir. İşlem örneği yoksa `—`; sıfır varsayılmaz. Grup satırı alt işlem yüzdelerini toplar; sistemin en yoğun motor değeriyle aynı tanım değildir. |
| Kullanılan VRAM | PDH `GPU Adapter Memory(*)\Dedicated Usage`, adaptör toplamı. Tümleşik GPU'daki paylaşılan RAM buna dahil değildir. |
| Toplam VRAM / GPU MHz | Henüz uygulanmadı; `—`. Üreticiye özel NVML/ADL/Intel API entegrasyonu yok. |
| Ağ toplamı | `GetIfTable2` bayt farkları; bağlı donanım adaptörlerinin toplamı. VPN, tünel ve loopback hariç; ilk örnek veya adaptör değişimi sonrası `—` olabilir. MB/s ondalık tabanlıdır. |
| Disk toplamı | PDH `PhysicalDisk(_Total)` okuma/yazma bayt/s. Önbelleğe alınmış uygulama I/O'su fiziksel disk aktarımıyla aynı değildir. Sayaç kapalı/erişilemezse `—`. |
| İşlem başına ağ / disk | Henüz uygulanmadı; `—`. ETW oturumu yoktur. `GetProcessIoCounters` ağ ve cihaz I/O'sunu da içerdiği için disk hızı olarak sunulmaz. |

Eksik ölçümler sahte sıfırla doldurulmaz. Grup içindeki herhangi bir işlemde ölçüm eksikse grubun o sütunu da `—` olur; kısmi toplam tam veri gibi sunulmaz. Program ölçüm döngüsünde PowerShell veya başka dış süreç başlatmaz. Ek yönetici yetkisi otomatik istenmez.

## Bilgisayarında kısa doğrulama

**Derleme başarısı donanım doğrulaması değildir.** GitHub çalıştırıcısı derleme/test/paketlemeyi kontrol eder; senin GPU sürücünü, disklerini ve adaptörlerini doğrulamaz.

1. Artifact ZIP'inin tamamını çıkar, EXE'yi normal kullanıcı olarak aç; ilk 2–3 örneği bekle. Görev Yöneticisi'ni aynı anda aç, güncelleme aralığını normale ayarla.
2. CPU yükü oluşturan normal bir iş çalıştır; toplam CPU ve işlem CPU'nun yükselip durunca azaldığını kontrol et. Aynı anda başlatılıp kapatılan uygulamaların listeye girip çıktığını, grupların sayılarının değiştiğini kontrol et.
3. RAM'i Performans → Bellek ile karşılaştır. İşlem RAM'i için Ayrıntılar'da **Çalışma kümesi (bellek)** sütununu aç; varsayılan özel bellek sütunu farklıdır.
4. Bir dosya indir; Görev Yöneticisi'ndeki ilgili fiziksel ağ adaptörüyle karşılaştır. Oradaki Mbit/s değerini **8'e bölerek MB/s** elde et. Sonra büyük bir dosyayı kopyala; disk okuma/yazma hareketini kontrol et. Önbellek ve örnekleme aralıkları nedeniyle birebir eşitlik bekleme.
5. GPU kullanan bir uygulama çalıştır. Görev Yöneticisi'nde en yoğun GPU motoruyla yönü/büyüklüğü karşılaştır; birden fazla GPU varsa bu uygulama tüm adaptörlerdeki en yoğun motoru gösterir. `—` için üzerine gelerek kapsamı oku; toplam VRAM, MHz ve işlem ağ/disk alanlarının bu sürümde `—` olması beklenir.
6. Dört modu sırayla aç; sıralama, açık gruplar ve ölçek korunsun. `+`, `−`, numpad ve `0` tuşlarını dene; küçük ekranda liste kaydırılabilsin. Kapatıp açınca mod ve ölçek geri gelsin. Küçültüp Görev Yöneticisi'nden uygulamanın kendi kaynak kullanımını izle.

İsteğe bağlı ham ölçüm kaydı (yaklaşık 6 saniye; JSON'daki `null` kullanılamayan ölçümdür):

```powershell
Start-Process -FilePath .\damned.exe -ArgumentList '--diagnose', 'damned-metrics.json' -Wait -WindowStyle Hidden
Get-Content .\damned-metrics.json
```

Tanılama örnekleri CPU/RAM/GPU/ağ/disk ham değerlerini ve uygulamanın kendi CPU/çalışma kümesini içerir. Bir ölçümün sayısal olması tek başına doğruluğunu kanıtlamaz; yukarıdaki yük/değişim karşılaştırması gerekir.

## Bu çalışma ortamında doğrulama durumu

- `Cargo.lock`, Rust stable MSVC ile üretildi; kilitli bağımlılıklar indirildi.
- Slint 1.18.1 ile arayüzün derleyici kontrolü yapıldı.
- Windows API toplayıcısı `windows-sys 0.61.2` ile bağımsız `cargo check` kontrolünden geçti.
- Yerel Windows x64 EXE, Rust stable LLVM (`x86_64-pc-windows-gnullvm`) ile release olarak derlendi ve çalıştırıldı. Yerel EXE `target/release/damned.exe`; bu LLVM derlemesinin yanında `libunwind.dll` gerekir. Bu makinede MSVC `link.exe` bulunmadığından MSVC doğrulaması GitHub'daki `windows-2025` iş akışında yapılır.
- Yerel altı örneklik tanılama CPU, fiziksel RAM, GPU, ağ ve disk sayaçlarından gerçek veri üretti; ilk hız örnekleri beklendiği gibi `null` kaldı. Bu kontrol tüm donanım ölçümlerinin doğruluğunun kanıtı değildir; yük altında karşılaştırma adımları yukarıdadır.
- GitHub deposu: [EYITS/damned](https://github.com/EYITS/damned). İndirilebilir MSVC artifact'ı yalnızca başarılı Actions çalışmasından sonra oluşur.
