# サンプル

## 参考リンク

- [ISM330DHCX](<https://www.st.com/ja/mems-and-sensors/ism330dhcx.html>)
- [ISM330DHCX Datasheet](<https://www.st.com/resource/en/datasheet/ism330dhcx.pdf>)
- [計測震度の算出方法](<https://www.jma.go.jp/jma/kishou/know/jishin/kyoshin/kaisetsu/calc_sindo.html>)
- [長周期地震動階級および長周期地震動階級関連解説表について](<https://www.jma.go.jp/jma/kishou/know/jishin/ltpgm_explain/about_level.html>)
- [地震動の予報業務許可等の申請の手引き](<https://www.jma.go.jp/jma/kishou/minkan/tebiki/jishin_tebiki.pdf>)
- [ingen084/seismometer](<https://github.com/ingen084/seismometer/>)
- [fleneindre/lpgm-calculator](<https://github.com/fleneindre/lpgm-calculator/>)

## 強震観測データ

- [強震観測データ](<https://www.data.jma.go.jp/eqev/data/kyoshin/jishin/index.html>)
- [強震データのフォーマット](<https://www.data.jma.go.jp/eqev/data/kyoshin/jishin/format.html>)
- 文字コードは`Shift_JIS`

### 加速度データ (gal, 100Hz)

- acc20240101000147254.csv
  - 2024年1月1日 16時10分 新潟県 上越市中ノ俣
  - 最大計測震度 4.7
  - 最大長周期地震動階級 2
- acc20240101000141329.csv
  - 2024年1月1日 16時10分 新潟県 糸魚川市一の宮
  - 最大計測震度 5.0
  - 最大長周期地震動階級 3
- acc20240101000165034.csv
  - 2024年1月1日 16時10分 新潟県 長岡市中之島
  - 最大計測震度 5.5
- acc20240101000147274.csv
  - 2024年1月1日 16時10分 石川県 珠洲市三崎町
  - 最大計測震度 6.1
  - 最大長周期地震動階級 4
- acc20240101000167016.csv
  - 2024年1月1日 16時10分 石川県 輪島市門前町走出
  - 最大計測震度 6.5


## 長周期地震動の観測結果

- [長周期地震動の観測結果](<https://www.data.jma.go.jp/eew/data/ltpgm_explain/data/past/past_list.html>)
- 文字コードは`Shift_JIS`

### 絶対加速度応答スペクトル (cm/s/s)

- 4725420240101161010_accsp.csv
  - acc20240101000147254.csv の絶対加速度応答スペクトル
- 4132920240101161010_accsp.csv
  - acc20240101000141329.csv の絶対加速度応答スペクトル
- 4727420240101160950_accsp.csv
  - acc20240101000147274.csv の絶対加速度応答スペクトル

### 絶対速度応答スペクトル (cm/s)

- 4725420240101161010_velsp.csv
  - acc20240101000147254.csv の絶対速度応答スペクトル
- 4132920240101161010_velsp.csv
  - acc20240101000141329.csv の絶対速度応答スペクトル
- 4727420240101160950_velsp.csv
  - acc20240101000147274.csv の絶対速度応答スペクトル
