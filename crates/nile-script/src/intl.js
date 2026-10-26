// `Intl` for desktop apps: NumberFormat (decimal, percent, currency, compact), DateTimeFormat, PluralRules, ListFormat,
// RelativeTimeFormat and Collator, plus the `toLocale*String` and `localeCompare` methods that use them. Written in
// JS over a small table of locale data instead of ICU (which would add megabytes), so it covers the locales listed
// in LOCALES below (en, fr, de, es, it, pt, nl, sv, ru, pl, tr, sw, ja, zh, ko, hi and the regions that differ: en-GB,
// en-IN, en-ZA, en-UG, en-KE, ...); other languages fall back to English wording with the number separators they
// share. Times use the system's local zone (or UTC with `timeZone: "UTC"`); other IANA zones are not available.
// Spliced into web.js's block (`g` is the global object, `call` reads the OS locale).
{
  const NNBSP = " ", NBSP = " ";

  // ---- locale data -------------------------------------------------------------------------------------
  const EN_MONTHS = ["January", "February", "March", "April", "May", "June", "July", "August", "September", "October", "November", "December"];
  const EN_DAYS = ["Sunday", "Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday"];
  const names = (months, days, shortMonths, shortDays) => ({
    months, days,
    monthsShort: shortMonths || months.map((m) => m.slice(0, 3)),
    daysShort: shortDays || days.map((d) => d.slice(0, 3)),
  });
  const LOCALES = {
    en: { dec: ".", grp: ",", cur: "pre", names: names(EN_MONTHS, EN_DAYS, EN_MONTHS.map((m) => m.slice(0, 3)), EN_DAYS.map((d) => d.slice(0, 3))), order: "mdy", sep: "/", h12: true, list: ["and", "or"], dateStyles: ["M/d/yy", "MMM d, y", "MMMM d, y", "EEEE, MMMM d, y"], text: "{M} {d}, {y}", wtext: "{w}, {M} {d}, {y}", ampm: ["AM", "PM"] },
    fr: { dec: ",", grp: " ", cur: "suf", names: names(["janvier", "février", "mars", "avril", "mai", "juin", "juillet", "août", "septembre", "octobre", "novembre", "décembre"], ["dimanche", "lundi", "mardi", "mercredi", "jeudi", "vendredi", "samedi"], ["janv.", "févr.", "mars", "avr.", "mai", "juin", "juil.", "août", "sept.", "oct.", "nov.", "déc."], ["dim.", "lun.", "mar.", "mer.", "jeu.", "ven.", "sam."]), order: "dmy", sep: "/", list: ["et", "ou"], dateStyles: ["dd/MM/y", "d MMM y", "d MMMM y", "EEEE d MMMM y"], text: "{d} {M} {y}", wtext: "{w} {d} {M} {y}", timeSep: ":" },
    de: { dec: ",", grp: ".", cur: "suf", names: names(["Januar", "Februar", "März", "April", "Mai", "Juni", "Juli", "August", "September", "Oktober", "November", "Dezember"], ["Sonntag", "Montag", "Dienstag", "Mittwoch", "Donnerstag", "Freitag", "Samstag"], ["Jan.", "Feb.", "März", "Apr.", "Mai", "Juni", "Juli", "Aug.", "Sept.", "Okt.", "Nov.", "Dez."], ["So.", "Mo.", "Di.", "Mi.", "Do.", "Fr.", "Sa."]), order: "dmy", sep: ".", list: ["und", "oder"], dateStyles: ["dd.MM.yy", "dd.MM.y", "d. MMMM y", "EEEE, d. MMMM y"], text: "{d}. {M} {y}", wtext: "{w}, {d}. {M} {y}" },
    es: { dec: ",", grp: ".", cur: "suf", minGrp: 2, names: names(["enero", "febrero", "marzo", "abril", "mayo", "junio", "julio", "agosto", "septiembre", "octubre", "noviembre", "diciembre"], ["domingo", "lunes", "martes", "miércoles", "jueves", "viernes", "sábado"], ["ene", "feb", "mar", "abr", "may", "jun", "jul", "ago", "sept", "oct", "nov", "dic"], ["dom", "lun", "mar", "mié", "jue", "vie", "sáb"]), order: "dmy", sep: "/", list: ["y", "o"], dateStyles: ["d/M/yy", "d MMM y", "d 'de' MMMM 'de' y", "EEEE, d 'de' MMMM 'de' y"], text: "{d} de {M} de {y}", wtext: "{w}, {d} de {M} de {y}" },
    it: { dec: ",", grp: ".", cur: "suf", minGrp: 2, names: names(["gennaio", "febbraio", "marzo", "aprile", "maggio", "giugno", "luglio", "agosto", "settembre", "ottobre", "novembre", "dicembre"], ["domenica", "lunedì", "martedì", "mercoledì", "giovedì", "venerdì", "sabato"], ["gen", "feb", "mar", "apr", "mag", "giu", "lug", "ago", "set", "ott", "nov", "dic"], ["dom", "lun", "mar", "mer", "gio", "ven", "sab"]), order: "dmy", sep: "/", list: ["e", "o"], dateStyles: ["dd/MM/yy", "d MMM y", "d MMMM y", "EEEE d MMMM y"], text: "{d} {M} {y}", wtext: "{w} {d} {M} {y}" },
    pt: { dec: ",", grp: ".", cur: "pre-sp", names: names(["janeiro", "fevereiro", "março", "abril", "maio", "junho", "julho", "agosto", "setembro", "outubro", "novembro", "dezembro"], ["domingo", "segunda-feira", "terça-feira", "quarta-feira", "quinta-feira", "sexta-feira", "sábado"], ["jan.", "fev.", "mar.", "abr.", "mai.", "jun.", "jul.", "ago.", "set.", "out.", "nov.", "dez."], ["dom.", "seg.", "ter.", "qua.", "qui.", "sex.", "sáb."]), order: "dmy", sep: "/", list: ["e", "ou"], dateStyles: ["dd/MM/y", "d 'de' MMM 'de' y", "d 'de' MMMM 'de' y", "EEEE, d 'de' MMMM 'de' y"], text: "{d} de {M} de {y}", wtext: "{w}, {d} de {M} de {y}" },
    nl: { dec: ",", grp: ".", cur: "pre-sp", names: names(["januari", "februari", "maart", "april", "mei", "juni", "juli", "augustus", "september", "oktober", "november", "december"], ["zondag", "maandag", "dinsdag", "woensdag", "donderdag", "vrijdag", "zaterdag"], ["jan", "feb", "mrt", "apr", "mei", "jun", "jul", "aug", "sep", "okt", "nov", "dec"], ["zo", "ma", "di", "wo", "do", "vr", "za"]), order: "dmy", sep: "-", list: ["en", "of"], dateStyles: ["dd-MM-y", "d MMM y", "d MMMM y", "EEEE d MMMM y"], text: "{d} {M} {y}", wtext: "{w} {d} {M} {y}" },
    sv: { dec: ",", grp: NBSP, cur: "suf", names: names(["januari", "februari", "mars", "april", "maj", "juni", "juli", "augusti", "september", "oktober", "november", "december"], ["söndag", "måndag", "tisdag", "onsdag", "torsdag", "fredag", "lördag"], ["jan.", "feb.", "mars", "apr.", "maj", "juni", "juli", "aug.", "sep.", "okt.", "nov.", "dec."], ["sön", "mån", "tis", "ons", "tors", "fre", "lör"]), order: "ymd", sep: "-", list: ["och", "eller"], dateStyles: ["y-MM-dd", "d MMM y", "d MMMM y", "EEEE d MMMM y"], text: "{d} {M} {y}", wtext: "{w} {d} {M} {y}" },
    ru: { dec: ",", grp: NBSP, cur: "suf", names: names(["января", "февраля", "марта", "апреля", "мая", "июня", "июля", "августа", "сентября", "октября", "ноября", "декабря"], ["воскресенье", "понедельник", "вторник", "среда", "четверг", "пятница", "суббота"], ["янв.", "февр.", "мар.", "апр.", "мая", "июн.", "июл.", "авг.", "сент.", "окт.", "нояб.", "дек."], ["вс", "пн", "вт", "ср", "чт", "пт", "сб"]), order: "dmy", sep: ".", list: ["и", "или"], dateStyles: ["dd.MM.y", "d MMM y 'г'.", "d MMMM y 'г'.", "EEEE, d MMMM y 'г'."], text: "{d} {M} {y} г.", wtext: "{w}, {d} {M} {y} г." },
    pl: { dec: ",", grp: NBSP, cur: "suf", minGrp: 2, names: names(["stycznia", "lutego", "marca", "kwietnia", "maja", "czerwca", "lipca", "sierpnia", "września", "października", "listopada", "grudnia"], ["niedziela", "poniedziałek", "wtorek", "środa", "czwartek", "piątek", "sobota"], ["sty", "lut", "mar", "kwi", "maj", "cze", "lip", "sie", "wrz", "paź", "lis", "gru"], ["niedz.", "pon.", "wt.", "śr.", "czw.", "pt.", "sob."]), order: "dmy", sep: ".", list: ["i", "lub"], dateStyles: ["d.MM.y", "d MMM y", "d MMMM y", "EEEE, d MMMM y"], text: "{d} {M} {y}", wtext: "{w}, {d} {M} {y}" },
    tr: { dec: ",", grp: ".", cur: "pre", names: names(["Ocak", "Şubat", "Mart", "Nisan", "Mayıs", "Haziran", "Temmuz", "Ağustos", "Eylül", "Ekim", "Kasım", "Aralık"], ["Pazar", "Pazartesi", "Salı", "Çarşamba", "Perşembe", "Cuma", "Cumartesi"], ["Oca", "Şub", "Mar", "Nis", "May", "Haz", "Tem", "Ağu", "Eyl", "Eki", "Kas", "Ara"], ["Paz", "Pzt", "Sal", "Çar", "Per", "Cum", "Cmt"]), order: "dmy", sep: ".", list: ["ve", "veya"], dateStyles: ["d.MM.y", "d MMM y", "d MMMM y", "d MMMM y EEEE"], text: "{d} {M} {y}", wtext: "{d} {M} {y} {w}" },
    sw: { dec: ".", grp: ",", cur: "pre-sp", names: names(["Januari", "Februari", "Machi", "Aprili", "Mei", "Juni", "Julai", "Agosti", "Septemba", "Oktoba", "Novemba", "Desemba"], ["Jumapili", "Jumatatu", "Jumanne", "Jumatano", "Alhamisi", "Ijumaa", "Jumamosi"], ["Jan", "Feb", "Mac", "Apr", "Mei", "Jun", "Jul", "Ago", "Sep", "Okt", "Nov", "Des"], ["Jumapili", "Jumatatu", "Jumanne", "Jumatano", "Alhamisi", "Ijumaa", "Jumamosi"].map((d) => d.slice(0, 3))), order: "dmy", sep: "/", list: ["na", "au"], dateStyles: ["dd/MM/y", "d MMM y", "d MMMM y", "EEEE, d MMMM y"], text: "{d} {M} {y}", wtext: "{w}, {d} {M} {y}" },
    ja: { dec: ".", grp: ",", cur: "pre", names: names(EN_MONTHS.map((_, i) => i + 1 + "月"), ["日曜日", "月曜日", "火曜日", "水曜日", "木曜日", "金曜日", "土曜日"], EN_MONTHS.map((_, i) => i + 1 + "月"), ["日", "月", "火", "水", "木", "金", "土"]), order: "ymd", sep: "/", list: ["、", "、"], dateStyles: ["y/MM/dd", "y年M月d日", "y年M月d日", "y年M月d日EEEE"], text: "{y}年{m}月{d}日", wtext: "{y}年{m}月{d}日{w}", ymd: true },
    zh: { dec: ".", grp: ",", cur: "pre", names: names(EN_MONTHS.map((_, i) => i + 1 + "月"), ["星期日", "星期一", "星期二", "星期三", "星期四", "星期五", "星期六"], EN_MONTHS.map((_, i) => i + 1 + "月"), ["周日", "周一", "周二", "周三", "周四", "周五", "周六"]), order: "ymd", sep: "/", list: ["和", "或"], dateStyles: ["y/M/d", "y年M月d日", "y年M月d日", "y年M月d日EEEE"], text: "{y}年{m}月{d}日", wtext: "{y}年{m}月{d}日{w}", ymd: true },
    ko: { dec: ".", grp: ",", cur: "pre", names: names(EN_MONTHS.map((_, i) => i + 1 + "월"), ["일요일", "월요일", "화요일", "수요일", "목요일", "금요일", "토요일"], EN_MONTHS.map((_, i) => i + 1 + "월"), ["일", "월", "화", "수", "목", "금", "토"]), order: "ymd", sep: ". ", list: ["및", "또는"], dateStyles: ["yy. M. d.", "y. M. d.", "y년 M월 d일", "y년 M월 d일 EEEE"], text: "{y}년 {m}월 {d}일", wtext: "{y}년 {m}월 {d}일 {w}", ymd: true },
    hi: { dec: ".", grp: ",", indian: true, cur: "pre", names: names(EN_MONTHS, EN_DAYS, EN_MONTHS.map((m) => m.slice(0, 3)), EN_DAYS.map((d) => d.slice(0, 3))), order: "dmy", sep: "/", list: ["और", "या"], dateStyles: ["d/M/yy", "d MMM y", "d MMMM y", "EEEE, d MMMM y"], text: "{d} {M} {y}", wtext: "{w}, {d} {M} {y}" },
  };
  // Regions that change something: grouping, the clock, the order of a numeric date, currency symbols.
  const REGIONS = {
    "en-GB": { base: "en", order: "dmy", h12: false, dateStyles: ["dd/MM/y", "d MMM y", "d MMMM y", "EEEE d MMMM y"], text: "{d} {M} {y}", wtext: "{w} {d} {M} {y}", ampm: ["am", "pm"] },
    "en-AU": { base: "en", order: "dmy", dateStyles: ["d/M/yy", "d MMM y", "d MMMM y", "EEEE, d MMMM y"], text: "{d} {M} {y}", wtext: "{w}, {d} {M} {y}", ampm: ["am", "pm"] },
    "en-IN": { base: "en", indian: true, order: "dmy", dateStyles: ["dd/MM/yy", "dd-MMM-y", "d MMMM y", "EEEE, d MMMM y"], text: "{d} {M} {y}", wtext: "{w}, {d} {M} {y}", ampm: ["am", "pm"], cur: { INR: "₹" } },
    "en-ZA": { base: "en", dec: ",", grp: NBSP, order: "ymd", sep: "/", cur: { ZAR: "R" }, dateStyles: ["y/MM/dd", "dd MMM y", "dd MMMM y", "EEEE, dd MMMM y"], text: "{d} {M} {y}", wtext: "{w}, {d} {M} {y}", h12: false },
    "en-UG": { base: "en", order: "dmy", h12: false, cur: { UGX: "USh" }, dateStyles: ["dd/MM/y", "d MMM y", "d MMMM y", "EEEE d MMMM y"], text: "{d} {M} {y}", wtext: "{w} {d} {M} {y}" },
    "en-KE": { base: "en", order: "dmy", h12: false, cur: { KES: "Ksh" }, dateStyles: ["dd/MM/y", "d MMM y", "d MMMM y", "EEEE d MMMM y"], text: "{d} {M} {y}", wtext: "{w} {d} {M} {y}" },
    "en-TZ": { base: "en", order: "dmy", h12: false, cur: { TZS: "TSh" }, dateStyles: ["dd/MM/y", "d MMM y", "d MMMM y", "EEEE d MMMM y"], text: "{d} {M} {y}", wtext: "{w} {d} {M} {y}" },
    "en-NG": { base: "en", order: "dmy", cur: { NGN: "₦" }, dateStyles: ["dd/MM/y", "d MMM y", "d MMMM y", "EEEE d MMMM y"], text: "{d} {M} {y}", wtext: "{w} {d} {M} {y}" },
    "en-GH": { base: "en", order: "dmy", cur: { GHS: "GH₵" }, dateStyles: ["dd/MM/y", "d MMM y", "d MMMM y", "EEEE d MMMM y"], text: "{d} {M} {y}", wtext: "{w} {d} {M} {y}" },
    "en-CA": { base: "en", order: "ymd", sep: "-", dateStyles: ["y-MM-dd", "MMM d, y", "MMMM d, y", "EEEE, MMMM d, y"], cur: { CAD: "$", USD: "US$" } },
    "fr-CA": { base: "fr", grp: NBSP, order: "ymd", sep: "-", dateStyles: ["y-MM-dd", "d MMM y", "d MMMM y", "EEEE d MMMM y"] },
    "fr-CH": { base: "fr", grp: "’", dec: "." },
    "de-CH": { base: "de", grp: "’", dec: ".", cur: "pre-sp" },
    "de-AT": { base: "de", grp: NBSP, cur: "pre-sp" },
    "es-MX": { base: "es", dec: ".", grp: ",", minGrp: 1, cur: "pre" },
    "es-US": { base: "es", dec: ".", grp: ",", minGrp: 1, cur: "pre" },
    "pt-PT": { base: "pt", grp: NBSP, minGrp: 2, cur: "suf" },
  };
  const SYMBOLS = { USD: "$", EUR: "€", GBP: "£", JPY: "¥", INR: "₹", CNY: "CN¥", KRW: "₩", ILS: "₪", VND: "₫", PHP: "₱", TWD: "NT$", HKD: "HK$", AUD: "A$", CAD: "CA$", NZD: "NZ$", MXN: "MX$", BRL: "R$", XAF: "FCFA", XOF: "F CFA" };
  const NARROW = { USD: "$", EUR: "€", GBP: "£", JPY: "¥", INR: "₹", CNY: "¥", KRW: "₩", AUD: "$", CAD: "$", NZD: "$", MXN: "$", BRL: "R$", HKD: "$", TWD: "$", NGN: "₦", KES: "KSh", UGX: "USh", ZAR: "R", GHS: "GH₵", TZS: "TSh", RUB: "₽", TRY: "₺", PLN: "zł", SEK: "kr", NOK: "kr", DKK: "kr", CHF: "CHF", ILS: "₪", VND: "₫", PHP: "₱" };
  const DIGITS = { JPY: 0, KRW: 0, UGX: 0, RWF: 0, XAF: 0, XOF: 0, VND: 0, CLP: 0, ISK: 0, PYG: 0, BIF: 0, DJF: 0, GNF: 0, KMF: 0, VUV: 0, BHD: 3, KWD: 3, OMR: 3, JOD: 3, TND: 3, LYD: 3, IQD: 0 };
  const CURRENCY_NAMES = { USD: ["US dollar", "US dollars"], EUR: ["euro", "euros"], GBP: ["British pound", "British pounds"], JPY: ["Japanese yen", "Japanese yen"], UGX: ["Ugandan shilling", "Ugandan shillings"], KES: ["Kenyan shilling", "Kenyan shillings"], TZS: ["Tanzanian shilling", "Tanzanian shillings"], NGN: ["Nigerian naira", "Nigerian nairas"], ZAR: ["South African rand", "South African rand"], INR: ["Indian rupee", "Indian rupees"], CNY: ["Chinese yuan", "Chinese yuan"], CAD: ["Canadian dollar", "Canadian dollars"], AUD: ["Australian dollar", "Australian dollars"], CHF: ["Swiss franc", "Swiss francs"], GHS: ["Ghanaian cedi", "Ghanaian cedis"], RWF: ["Rwandan franc", "Rwandan francs"] };

  const rangeError = (m) => new RangeError(m);
  /** { tag, lang, region, data } for a locale argument (string, array or undefined). */
  const resolve = (locales) => {
    let want = Array.isArray(locales) ? locales[0] : locales;
    if (want === undefined || want === null) want = defaultLocale();
    want = String(want);
    if (!/^[A-Za-z]{2,3}([-_][A-Za-z0-9]{2,8})*$/.test(want)) throw rangeError("Incorrect locale information provided");
    const parts = want.replace(/_/g, "-").split("-");
    const lang = parts[0].toLowerCase();
    const region = parts.find((p, i) => i > 0 && /^[A-Za-z]{2}$/.test(p));
    const tag = lang + (region ? "-" + region.toUpperCase() : "");
    const baseLang = LOCALES[lang] ? lang : "en";
    const merged = Object.assign({}, LOCALES[baseLang]);
    const reg = REGIONS[tag];
    if (reg) Object.assign(merged, reg);
    // Without a region of its own, the clock follows the language: only English and a few use 12 hours.
    if (merged.h12 === undefined) merged.h12 = false;
    return { tag: tag, lang: lang, region: region ? region.toUpperCase() : "", data: merged, known: !!LOCALES[lang] };
  };
  const defaultLocale = () => (g.navigator && g.navigator.language) || "en-US";

  // ---- decimal arithmetic on strings (no float surprises: 1.005 rounds to 1.01) --------------------------
  /** Digits and the position of the decimal point for a finite non-negative number: { d: "1005", e: 1 } is 1.005. */
  const decimal = (x) => {
    let s = String(x);
    let exp = 0;
    const m = /^(\d+)(?:\.(\d+))?(?:e([+-]\d+))?$/i.exec(s);
    if (!m) return { d: "0", e: 1 };
    exp = m[3] ? Number(m[3]) : 0;
    let digits = m[1] + (m[2] || "");
    let point = m[1].length + exp;
    const lead = /^0*/.exec(digits)[0].length;
    if (lead === digits.length) return { d: "0", e: 1 };
    digits = digits.slice(lead);
    point -= lead;
    return { d: digits.replace(/0+$/, "") || "0", e: point };
  };
  /** Keep `keep` leading digits of `dec` ({ d, e }), rounding half away from zero; { d, e } again (d may be "" for zero). */
  const roundAt = (dec, keep) => {
    let { d, e } = dec;
    if (d === "0") return { d: "", e: 1 };
    if (keep < 0) return { d: "", e: 1 };
    let kept = d.slice(0, keep).padEnd(keep, "0");
    if (d.charAt(keep) >= "5") {
      const arr = kept.split("");
      let i = arr.length - 1;
      while (i >= 0 && arr[i] === "9") { arr[i] = "0"; i--; }
      if (i >= 0) { arr[i] = String(Number(arr[i]) + 1); kept = arr.join(""); } else { kept = "1" + arr.join(""); e += 1; }
    }
    return { d: kept, e: e };
  };
  /** { int, frac } digit strings of a rounded { d, e }. */
  const splitDigits = ({ d, e }) => {
    if (d === "") return { int: "0", frac: "" };
    if (e <= 0) return { int: "0", frac: "0".repeat(-e) + d };
    return { int: d.slice(0, e).padEnd(e, "0"), frac: d.slice(e) };
  };
  /** Round to `frac` digits after the point (so 1.005 gives 1.01: the shortest decimal form is rounded, not the binary one). */
  const roundFraction = (dec, frac) => {
    const r = splitDigits(roundAt(dec, dec.e + frac));
    return { int: r.int, frac: r.frac.padEnd(frac, "0").slice(0, Math.max(frac, r.frac.length)) };
  };
  /** Round to `sig` significant digits. */
  const roundSignificant = (dec, sig) => splitDigits(roundAt(dec, sig));

  // ---- NumberFormat ----------------------------------------------------------------------------------------
  const groupInt = (int, loc, useGrouping) => {
    if (useGrouping === false || useGrouping === "false") return int;
    const min = (loc.data.minGrp || 1) + 3;
    if (int.length < min) return int;
    const sep = loc.data.grp;
    if (loc.data.indian) {
      const last3 = int.slice(-3);
      let rest = int.slice(0, -3);
      const parts = [];
      while (rest.length > 2) { parts.unshift(rest.slice(-2)); rest = rest.slice(0, -2); }
      if (rest) parts.unshift(rest);
      return parts.concat(last3).join(sep);
    }
    const out = [];
    for (let i = int.length; i > 0; i -= 3) out.unshift(int.slice(Math.max(0, i - 3), i));
    return out.join(sep);
  };
  const COMPACT = [[1e12, "T", " trillion"], [1e9, "B", " billion"], [1e6, "M", " million"], [1e3, "K", " thousand"]];

  class NumberFormat {
    constructor(locales, options) {
      const o = options === undefined ? {} : Object(options);
      this._loc = resolve(locales);
      const style = o.style === undefined ? "decimal" : String(o.style);
      if (!["decimal", "percent", "currency", "unit"].includes(style)) throw rangeError("Invalid style: " + style);
      let currency;
      if (style === "currency") {
        if (o.currency === undefined) throw new TypeError("Currency code is required with currency style.");
        currency = String(o.currency).toUpperCase();
        if (!/^[A-Z]{3}$/.test(currency)) throw rangeError("Invalid currency code : " + o.currency);
      }
      const cd = style === "currency" ? (DIGITS[currency] !== undefined ? DIGITS[currency] : 2) : 0;
      const hasMin = o.minimumFractionDigits !== undefined, hasMax = o.maximumFractionDigits !== undefined;
      let minF = hasMin ? Number(o.minimumFractionDigits) : undefined, maxF = hasMax ? Number(o.maximumFractionDigits) : undefined;
      for (const v of [minF, maxF]) if (v !== undefined && !(v >= 0 && v <= 100)) throw rangeError("maximumFractionDigits value is out of range.");
      const dMin = style === "currency" ? cd : 0, dMax = style === "currency" ? cd : style === "percent" ? 0 : 3;
      if (minF === undefined) minF = hasMax ? Math.min(dMin, maxF) : dMin;
      if (maxF === undefined) maxF = Math.max(dMax, minF);
      if (minF > maxF) throw rangeError("maximumFractionDigits value is out of range.");
      this._o = {
        style, currency,
        currencyDisplay: o.currencyDisplay === undefined ? "symbol" : String(o.currencyDisplay),
        minInt: o.minimumIntegerDigits === undefined ? 1 : Number(o.minimumIntegerDigits),
        minF, maxF,
        minS: o.minimumSignificantDigits === undefined ? undefined : Number(o.minimumSignificantDigits),
        maxS: o.maximumSignificantDigits === undefined ? undefined : Number(o.maximumSignificantDigits),
        useGrouping: o.useGrouping === undefined ? true : o.useGrouping,
        notation: o.notation === undefined ? "standard" : String(o.notation),
        compactDisplay: o.compactDisplay === undefined ? "short" : String(o.compactDisplay),
        signDisplay: o.signDisplay === undefined ? "auto" : String(o.signDisplay),
        unit: o.unit, unitDisplay: o.unitDisplay === undefined ? "short" : String(o.unitDisplay),
      };
      if (this._o.maxS !== undefined && this._o.minS === undefined) this._o.minS = 1;
      if (this._o.minS !== undefined && this._o.maxS === undefined) this._o.maxS = 21;
      this._o.hasFraction = hasMin || hasMax;
    }
    get format() {
      const f = (n) => this._format(n);
      Object.defineProperty(this, "format", { value: f });
      return f;
    }
    formatToParts(n) {
      return this._parts(n);
    }
    resolvedOptions() {
      const o = this._o;
      const r = { locale: this._loc.tag, numberingSystem: "latn", style: o.style };
      if (o.style === "currency") { r.currency = o.currency; r.currencyDisplay = o.currencyDisplay; r.currencySign = "standard"; }
      r.minimumIntegerDigits = o.minInt;
      if (o.minS !== undefined) { r.minimumSignificantDigits = o.minS; r.maximumSignificantDigits = o.maxS; } else { r.minimumFractionDigits = o.minF; r.maximumFractionDigits = o.maxF; }
      r.useGrouping = o.useGrouping === false ? false : "auto";
      r.notation = o.notation; r.signDisplay = o.signDisplay; r.roundingMode = "halfExpand";
      return r;
    }
    static supportedLocalesOf(locales) {
      return (Array.isArray(locales) ? locales : locales === undefined ? [] : [locales]).filter((l) => LOCALES[String(l).split("-")[0].toLowerCase()]);
    }
    _format(n) {
      return this._parts(n).map((p) => p.value).join("");
    }
    _parts(n) {
      const o = this._o, loc = this._loc, data = loc.data;
      let x = typeof n === "bigint" ? Number(n) : Number(n);
      const parts = [];
      const push = (type, value) => { if (value !== "") parts.push({ type, value }); };
      const negative = x < 0 || Object.is(x, -0);
      const sign = (() => {
        const s = o.signDisplay;
        if (Number.isNaN(x)) return "";
        if (s === "never") return "";
        if (s === "always") return negative ? "-" : "+";
        if (s === "exceptZero") return x === 0 ? "" : negative ? "-" : "+";
        if (s === "negative") return x < 0 ? "-" : "";
        return negative ? "-" : "";
      })();
      let body = [];
      if (Number.isNaN(x)) body = [{ type: "nan", value: "NaN" }];
      else if (!Number.isFinite(x)) body = [{ type: "infinity", value: "∞" }];
      else {
        let abs = Math.abs(x);
        if (o.style === "percent") abs = abs * 100;
        let suffix = "";
        let compactKey = null;
        if (o.notation === "compact") {
          for (const [limit, shortS, longS] of COMPACT) {
            if (abs >= limit) { abs = abs / limit; suffix = o.compactDisplay === "long" ? longS : shortS; compactKey = limit; break; }
          }
        }
        let r;
        if (o.maxS !== undefined) {
          r = roundSignificant(decimal(abs), o.maxS);
          let frac = r.frac;
          const sig = () => (r.int === "0" ? frac.replace(/^0+/, "").length : r.int.length + frac.length);
          while (frac.endsWith("0") && sig() > o.minS) frac = frac.slice(0, -1);
          while (sig() < o.minS) frac += "0";
          r = { int: r.int, frac: frac };
        } else if (o.notation === "compact" && !o.hasFraction) {
          // compact notation keeps two significant digits below 100 and whole numbers above
          const dec = decimal(abs);
          r = abs < 100 ? roundSignificant(dec, 2) : roundFraction(dec, 0);
          let frac = r.frac.replace(/0+$/, "");
          r = { int: r.int, frac: frac };
        } else {
          r = roundFraction(decimal(abs), o.maxF);
          let frac = r.frac;
          while (frac.length > o.minF && frac.endsWith("0")) frac = frac.slice(0, -1);
          r = { int: r.int, frac: frac };
        }
        const int = r.int.padStart(o.minInt, "0");
        const grouped = groupInt(int, loc, o.notation === "compact" && int.length < 5 ? false : o.useGrouping);
        const groups = grouped.split(data.grp === "" ? "\u0000" : data.grp);
        groups.forEach((g2, i) => {
          if (i > 0) body.push({ type: "group", value: data.grp });
          body.push({ type: "integer", value: g2 });
        });
        if (r.frac) {
          body.push({ type: "decimal", value: data.dec });
          body.push({ type: "fraction", value: r.frac });
        }
        if (suffix) body.push({ type: "compact", value: suffix.trim() });
        if (suffix.startsWith(" ")) { const c = body.pop(); body.push({ type: "literal", value: " " }); body.push(c); }
        if (o.style === "percent") {
          if (["en", "ja", "zh", "ko", "hi", "sw", "tr"].includes(loc.lang) && loc.lang !== "tr") body.push({ type: "percentSign", value: "%" });
          else if (loc.lang === "tr") body.unshift({ type: "percentSign", value: "%" });
          else { body.push({ type: "literal", value: NBSP }); body.push({ type: "percentSign", value: "%" }); }
        }
      }
      const signPart = sign ? [{ type: sign === "-" ? "minusSign" : "plusSign", value: sign }] : [];
      if (o.style === "currency") {
        const cur = o.currency;
        let text;
        const region = (REGIONS[loc.tag] && REGIONS[loc.tag].cur && typeof REGIONS[loc.tag].cur === "object" && REGIONS[loc.tag].cur[cur]);
        if (o.currencyDisplay === "code") text = cur;
        else if (o.currencyDisplay === "narrowSymbol") text = NARROW[cur] || cur;
        else if (o.currencyDisplay === "name") text = null;
        else text = region || (loc.lang === "ja" && cur === "JPY" ? "￥" : loc.lang === "zh" && cur === "CNY" ? "¥" : SYMBOLS[cur]) || cur;
        if (text === null) {
          const nm = CURRENCY_NAMES[cur] || [cur, cur];
          const plural = body.length === 1 && body[0].value === "1" ? nm[0] : nm[1];
          return [...signPart, ...body, { type: "literal", value: " " }, { type: "currency", value: plural }];
        }
        const place = typeof data.cur === "string" ? data.cur : "pre";
        const letter = /[A-Za-zÀ-ɏ]$/.test(text);
        const first = /^[A-Za-z]/.test(text);
        if (place === "suf") return [...signPart, ...body, { type: "literal", value: NBSP }, { type: "currency", value: text }];
        if (place === "pre-sp") return [{ type: "currency", value: text }, { type: "literal", value: NBSP }, ...signPart, ...body];
        const gap = letter || first ? [{ type: "literal", value: NBSP }] : [];
        return [...signPart, { type: "currency", value: text }, ...gap, ...body];
      }
      if (o.style === "unit" && o.unit) {
        const u = UNITS[o.unit];
        const text = u ? (o.unitDisplay === "long" ? u[1] : u[0]) : o.unit;
        return [...signPart, ...body, { type: "literal", value: o.unitDisplay === "narrow" ? "" : " " }, { type: "unit", value: text }];
      }
      return [...signPart, ...body];
    }
  }
  const UNITS = { kilometer: ["km", "kilometers"], meter: ["m", "meters"], centimeter: ["cm", "centimeters"], mile: ["mi", "miles"], kilogram: ["kg", "kilograms"], gram: ["g", "grams"], pound: ["lb", "pounds"], liter: ["L", "liters"], celsius: ["°C", "degrees Celsius"], fahrenheit: ["°F", "degrees Fahrenheit"], percent: ["%", "percent"], second: ["sec", "seconds"], minute: ["min", "minutes"], hour: ["hr", "hours"], day: ["days", "days"], byte: ["byte", "bytes"], kilobyte: ["kB", "kilobytes"], megabyte: ["MB", "megabytes"], gigabyte: ["GB", "gigabytes"] };

  // ---- DateTimeFormat --------------------------------------------------------------------------------------
  const pad2 = (n) => (n < 10 ? "0" : "") + n;
  const fieldsOf = (date, utc) => ({
    y: utc ? date.getUTCFullYear() : date.getFullYear(),
    m: (utc ? date.getUTCMonth() : date.getMonth()),
    d: utc ? date.getUTCDate() : date.getDate(),
    w: utc ? date.getUTCDay() : date.getDay(),
    h: utc ? date.getUTCHours() : date.getHours(),
    mi: utc ? date.getUTCMinutes() : date.getMinutes(),
    s: utc ? date.getUTCSeconds() : date.getSeconds(),
    ms: utc ? date.getUTCMilliseconds() : date.getMilliseconds(),
  });
  /** Expand a CLDR-style pattern ("EEEE, MMMM d, y") for the given fields. */
  const applyPattern = (pattern, f, loc) => {
    const n = loc.data.names;
    return pattern.replace(/'([^']*)'|EEEE|EEE|MMMM|MMM|MM|M|dd|d|yy|y/g, (t, lit) => {
      if (lit !== undefined) return lit;
      switch (t) {
        case "EEEE": return n.days[f.w];
        case "EEE": return n.daysShort[f.w];
        case "MMMM": return n.months[f.m];
        case "MMM": return n.monthsShort[f.m];
        case "MM": return pad2(f.m + 1);
        case "M": return String(f.m + 1);
        case "dd": return pad2(f.d);
        case "d": return String(f.d);
        case "yy": return pad2(f.y % 100);
        default: return String(f.y);
      }
    });
  };
  const timeText = (f, loc, o) => {
    const h12 = o.hour12 !== undefined ? o.hour12 : o.hourCycle ? /h1[12]/.test(o.hourCycle) : loc.data.h12;
    const parts = [];
    let h = f.h;
    let suffix = "";
    if (h12) {
      suffix = f.h < 12 ? (loc.data.ampm || ["AM", "PM"])[0] : (loc.data.ampm || ["AM", "PM"])[1];
      h = f.h % 12 === 0 ? 12 : f.h % 12;
    }
    const hourText = o.hour === "2-digit" || (!h12 && o.hour !== "numeric" && o.hour !== undefined) || (!h12 && o.timeStyle) ? pad2(h) : String(h);
    if (o.hour !== undefined || o.timeStyle !== undefined) parts.push(!h12 && o.hour === "numeric" && !o.timeStyle ? pad2(h) : hourText);
    if (o.minute !== undefined || o.timeStyle !== undefined) parts.push(pad2(f.mi));
    if (o.second !== undefined || o.timeStyle === "medium" || o.timeStyle === "long" || o.timeStyle === "full") parts.push(pad2(f.s));
    let text = parts.join(":");
    // a lone minute or second is just the number
    if (o.hour === undefined && !o.timeStyle) text = o.minute !== undefined && o.second === undefined ? String(f.mi) : text;
    if (h12 && (o.hour !== undefined || o.timeStyle !== undefined)) text += NNBSP + suffix;
    return text;
  };
  class DateTimeFormat {
    constructor(locales, options) {
      const o = options === undefined ? {} : Object(options);
      this._loc = resolve(locales);
      this._o = Object.assign({}, o);
      if (o.timeZone !== undefined) {
        const tz = String(o.timeZone);
        if (!/^(utc|gmt|etc\/utc|etc\/gmt)$/i.test(tz)) {
          // Only UTC and the local zone are known; anything else formats in local time.
          this._o.timeZone = undefined;
          this._tzUnknown = tz;
        } else this._o.timeZone = "UTC";
      }
      for (const k of ["dateStyle", "timeStyle"]) if (o[k] !== undefined && !["full", "long", "medium", "short"].includes(o[k])) throw rangeError("Invalid " + k + ": " + o[k]);
      const hasComponent = ["weekday", "year", "month", "day", "hour", "minute", "second"].some((k) => o[k] !== undefined);
      if ((o.dateStyle || o.timeStyle) && hasComponent) throw new TypeError("Can't set option when dateStyle or timeStyle is used");
      if (!o.dateStyle && !o.timeStyle && !hasComponent) {
        // the default is the numeric date; toLocaleTimeString and friends add what they need
        this._o.year = "numeric"; this._o.month = "numeric"; this._o.day = "numeric";
        this._defaulted = true;
      }
    }
    get format() {
      const f = (d) => this._format(d);
      Object.defineProperty(this, "format", { value: f });
      return f;
    }
    formatToParts(d) {
      return [{ type: "literal", value: this._format(d) }];
    }
    formatRange(a, b) {
      return this._format(a) + " – " + this._format(b);
    }
    resolvedOptions() {
      const o = this._o;
      let tz = o.timeZone === "UTC" ? "UTC" : (g.process && g.process.env && g.process.env.TZ) || localZoneName();
      const r = { locale: this._loc.tag, calendar: "gregory", numberingSystem: "latn", timeZone: tz };
      for (const k of ["weekday", "year", "month", "day", "hour", "minute", "second", "dateStyle", "timeStyle"]) if (o[k] !== undefined) r[k] = o[k];
      return r;
    }
    static supportedLocalesOf(locales) {
      return NumberFormat.supportedLocalesOf(locales);
    }
    _format(date) {
      let d = date === undefined ? new Date() : date instanceof Date ? date : new Date(date);
      if (Number.isNaN(d.getTime())) throw rangeError("Invalid time value");
      const o = this._o, loc = this._loc, data = loc.data;
      const f = fieldsOf(d, o.timeZone === "UTC");
      const pieces = [];
      const wantDate = o.dateStyle !== undefined || ["weekday", "year", "month", "day"].some((k) => o[k] !== undefined);
      const wantTime = o.timeStyle !== undefined || ["hour", "minute", "second"].some((k) => o[k] !== undefined);
      if (o.dateStyle !== undefined) {
        const i = { short: 0, medium: 1, long: 2, full: 3 }[o.dateStyle];
        pieces.push(applyPattern(data.dateStyles[i], f, loc));
      } else if (wantDate) {
        pieces.push(componentDate(o, f, loc));
      }
      if (wantTime) {
        const time = timeText(f, loc, o);
        const long = o.timeStyle === "long" || o.timeStyle === "full";
        pieces.push(time + (long ? " " + zoneLabel(d, o.timeZone === "UTC") : ""));
      }
      if (pieces.length === 2) {
        const dateLong = o.dateStyle === "long" || o.dateStyle === "full" || o.month === "long" || o.month === "short";
        const joiner = loc.lang === "en" ? (dateLong ? " 'at' " : ", ") : loc.lang === "de" ? (dateLong ? " um " : ", ") : loc.lang === "fr" ? (dateLong ? " à " : " ") : ", ";
        return pieces[0] + joiner.replace(/'at'/, "at") + pieces[1];
      }
      return pieces.join("");
    }
  }
  const zoneLabel = (date, utc) => {
    if (utc) return "UTC";
    const off = -date.getTimezoneOffset();
    if (off === 0) return "UTC";
    const sign = off < 0 ? "-" : "+";
    const abs = Math.abs(off);
    return "GMT" + sign + Math.floor(abs / 60) + (abs % 60 ? ":" + pad2(abs % 60) : "");
  };
  const localZoneName = () => {
    const off = -new Date().getTimezoneOffset();
    if (off === 0) return "UTC";
    return "Etc/GMT" + (off < 0 ? "+" : "-") + Math.abs(off / 60);
  };
  /** Date text from component options (weekday/year/month/day). */
  const componentDate = (o, f, loc) => {
    const data = loc.data, n = data.names;
    const year = o.year === undefined ? null : o.year === "2-digit" ? pad2(f.y % 100) : String(f.y);
    const day = o.day === undefined ? null : o.day === "2-digit" ? pad2(f.d) : String(f.d);
    const weekday = o.weekday === undefined ? null : o.weekday === "long" ? n.days[f.w] : o.weekday === "short" ? n.daysShort[f.w] : n.days[f.w].charAt(0).toUpperCase();
    const textual = o.month === "long" || o.month === "short" || o.month === "narrow";
    if (textual) {
      const month = o.month === "long" ? n.months[f.m] : o.month === "short" ? n.monthsShort[f.m] : n.months[f.m].charAt(0).toUpperCase();
      // month + year only, or month + day only, read differently from a full date
      let text;
      if (year && day) text = (weekday ? data.wtext : data.text);
      else if (year) text = data.ymd ? "{y}年{m}月" : loc.lang === "en" ? "{M} {y}" : loc.lang === "de" ? "{M} {y}" : "{M} {y}";
      else if (day) text = data.ymd ? "{m}月{d}日" : loc.lang === "en" && !(loc.tag === "en-US") ? "{d} {M}" : loc.lang === "en" ? "{M} {d}" : loc.lang === "de" ? "{d}. {M}" : "{d} {M}";
      else text = "{M}";
      const monthNum = String(f.m + 1);
      let out = text.replace("{w}", weekday || "").replace("{M}", month).replace("{m}", monthNum).replace("{d}", day || "").replace("{y}", year || "");
      if (weekday && !/\{w\}/.test(text) && !(year && day)) out = weekday + ", " + out;
      return out.trim();
    }
    // numeric month
    const month = o.month === undefined ? null : o.month === "2-digit" || (o.month === "numeric" && data.order === "dmy" && o.day === "2-digit") ? pad2(f.m + 1) : String(f.m + 1);
    const pieces = { y: year, M: month, d: day };
    // numeric dates take the 2-digit form where the locale's short pattern does (dd/MM/y, dd.MM.y)
    const pat = data.dateStyles[0];
    const twoDigit = /dd/.test(pat) && /MM/.test(pat);
    if (twoDigit && o.year !== undefined && o.month === "numeric" && o.day === "numeric") { pieces.M = pad2(f.m + 1); pieces.d = pad2(f.d); if (pat.indexOf("yy") >= 0 && pat.indexOf("y", pat.indexOf("yy") + 2) < 0) pieces.y = String(f.y); }
    const orderKeys = data.order === "mdy" ? ["M", "d", "y"] : data.order === "ymd" ? ["y", "M", "d"] : ["d", "M", "y"];
    const sep = data.sep;
    const text = orderKeys.map((k) => pieces[k]).filter((v) => v !== null && v !== undefined).join(sep);
    return (weekday ? weekday + ", " : "") + text;
  };

  // ---- PluralRules, ListFormat, RelativeTimeFormat -------------------------------------------------------------
  const pluralCardinal = (lang, n) => {
    const i = Math.floor(Math.abs(n)), isInt = Number.isInteger(n);
    switch (lang) {
      case "fr": case "pt": return i === 0 || i === 1 ? "one" : "other";
      case "ja": case "zh": case "ko": case "tr": return lang === "tr" ? (n === 1 ? "one" : "other") : "other";
      case "ru": {
        if (!isInt) return "other";
        const m10 = n % 10, m100 = n % 100;
        if (m10 === 1 && m100 !== 11) return "one";
        if (m10 >= 2 && m10 <= 4 && !(m100 >= 12 && m100 <= 14)) return "few";
        return "many";
      }
      case "pl": {
        if (!isInt) return "other";
        if (n === 1) return "one";
        const m10 = n % 10, m100 = n % 100;
        if (m10 >= 2 && m10 <= 4 && !(m100 >= 12 && m100 <= 14)) return "few";
        return "many";
      }
      case "hi": return n >= 0 && n <= 1 ? "one" : "other";
      default: return n === 1 && isInt ? "one" : "other";
    }
  };
  class PluralRules {
    constructor(locales, options) {
      this._loc = resolve(locales);
      this._type = options && options.type === "ordinal" ? "ordinal" : "cardinal";
    }
    select(n) {
      n = Number(n);
      if (this._type === "ordinal") {
        if (this._loc.lang !== "en") return "other";
        const m10 = n % 10, m100 = n % 100;
        if (m10 === 1 && m100 !== 11) return "one";
        if (m10 === 2 && m100 !== 12) return "two";
        if (m10 === 3 && m100 !== 13) return "few";
        return "other";
      }
      return pluralCardinal(this._loc.lang, n);
    }
    resolvedOptions() {
      return { locale: this._loc.tag, type: this._type };
    }
    static supportedLocalesOf(l) { return NumberFormat.supportedLocalesOf(l); }
  }
  class ListFormat {
    constructor(locales, options) {
      const o = options || {};
      this._loc = resolve(locales);
      this._type = o.type === "disjunction" ? 1 : o.type === "unit" ? 2 : 0;
      this._style = o.style || "long";
    }
    format(list) {
      const items = Array.from(list, String);
      const word = this._type === 2 ? "" : this._loc.data.list[this._type];
      if (items.length === 0) return "";
      if (items.length === 1) return items[0];
      const en = this._loc.lang === "en";
      if (this._type === 2) return items.join(this._style === "narrow" ? " " : ", ");
      if (items.length === 2) return items[0] + " " + (this._style === "short" && this._type === 0 && en ? "&" : word) + " " + items[1];
      // English lists take the serial comma, except the British-style regions.
      const oxford = en && !/^en-(GB|AU|IN|ZA|UG|KE|TZ|NG|GH|NZ)$/.test(this._loc.tag) ? "," : "";
      const last = this._style === "short" && this._type === 0 && en ? "&" : word;
      return items.slice(0, -1).join(", ") + oxford + " " + last + " " + items[items.length - 1];
    }
    formatToParts(list) {
      return [{ type: "literal", value: this.format(list) }];
    }
    resolvedOptions() { return { locale: this._loc.tag, type: ["conjunction", "disjunction", "unit"][this._type], style: this._style }; }
    static supportedLocalesOf(l) { return NumberFormat.supportedLocalesOf(l); }
  }
  const REL_UNITS = ["second", "minute", "hour", "day", "week", "month", "quarter", "year"];
  class RelativeTimeFormat {
    constructor(locales, options) {
      const o = options || {};
      this._loc = resolve(locales);
      this._numeric = o.numeric === "auto" ? "auto" : "always";
      this._style = o.style || "long";
    }
    format(value, unit) {
      let u = String(unit).replace(/s$/, "");
      if (!REL_UNITS.includes(u)) throw rangeError("Invalid unit argument for format() '" + unit + "'");
      const v = Number(value);
      if (this._numeric === "auto") {
        const special = { day: { "-1": "yesterday", 0: "today", 1: "tomorrow" }, year: { "-1": "last year", 0: "this year", 1: "next year" }, month: { "-1": "last month", 0: "this month", 1: "next month" }, week: { "-1": "last week", 0: "this week", 1: "next week" }, hour: { 0: "this hour" }, minute: { 0: "this minute" }, second: { 0: "now" }, quarter: { "-1": "last quarter", 0: "this quarter", 1: "next quarter" } };
        const s = special[u] && special[u][String(v)];
        if (s && !Object.is(v, -0)) return s;
      }
      const abs = Math.abs(v);
      const short = { second: "sec.", minute: "min.", hour: "hr.", day: "day", week: "wk.", month: "mo.", quarter: "qtr.", year: "yr." };
      let label = this._style === "long" ? u : short[u];
      const plural = abs !== 1;
      if (this._style === "long" || ["day"].includes(u)) label = u + (plural ? "s" : "");
      else if (u === "quarter") label = plural ? "qtrs." : "qtr.";
      const num = new NumberFormat(this._loc.tag).format(abs);
      const future = v > 0 || Object.is(v, +0);
      return future ? "in " + num + " " + label : num + " " + label + " ago";
    }
    resolvedOptions() { return { locale: this._loc.tag, style: this._style, numeric: this._numeric }; }
    static supportedLocalesOf(l) { return NumberFormat.supportedLocalesOf(l); }
  }

  // ---- Collator -------------------------------------------------------------------------------------------------
  const strip = (s) => s.normalize("NFD").replace(/[̀-ͯ]/g, "");
  const chunks = (s) => s.match(/\d+|\D+/g) || [];
  class Collator {
    constructor(locales, options) {
      const o = options || {};
      this._loc = resolve(locales);
      this._o = { numeric: !!o.numeric, sensitivity: o.sensitivity || "variant", caseFirst: o.caseFirst || "false", ignorePunctuation: !!o.ignorePunctuation, usage: o.usage || "sort" };
      if (!["base", "accent", "case", "variant"].includes(this._o.sensitivity)) throw rangeError("Value " + o.sensitivity + " out of range for Intl.Collator options property sensitivity");
    }
    get compare() {
      const f = (a, b) => this._compare(String(a), String(b));
      Object.defineProperty(this, "compare", { value: f });
      return f;
    }
    resolvedOptions() {
      return { locale: this._loc.tag, usage: this._o.usage, sensitivity: this._o.sensitivity, ignorePunctuation: this._o.ignorePunctuation, collation: "default", numeric: this._o.numeric, caseFirst: this._o.caseFirst };
    }
    static supportedLocalesOf(l) { return NumberFormat.supportedLocalesOf(l); }
    _compare(a, b) {
      const o = this._o;
      if (o.ignorePunctuation) { a = a.replace(/[\s\p{P}]/gu, ""); b = b.replace(/[\s\p{P}]/gu, ""); }
      // primary: letters without accents or case, digits compared as numbers when asked
      const base = (s) => strip(s).toLowerCase();
      const pa = base(a), pb = base(b);
      let c = o.numeric ? this._numericCompare(pa, pb) : primary(pa, pb);
      if (c !== 0) return c;
      if (o.sensitivity === "base") return 0;
      // secondary: accents
      if (o.sensitivity === "accent" || o.sensitivity === "variant") {
        const sa = a.normalize("NFD").toLowerCase(), sb = b.normalize("NFD").toLowerCase();
        if (sa !== sb) return sa < sb ? -1 : 1;
      }
      // tertiary: case (lower before upper unless caseFirst says otherwise)
      if (o.sensitivity === "case" || o.sensitivity === "variant") {
        const ca = strip(a), cb = strip(b);
        if (ca !== cb) {
          for (let i = 0; i < Math.min(ca.length, cb.length); i++) {
            if (ca[i] !== cb[i]) {
              const lowerA = ca[i] === ca[i].toLowerCase();
              const first = o.caseFirst === "upper" ? -1 : 1;
              return lowerA ? -first : first;
            }
          }
        }
      }
      return 0;
    }
    _numericCompare(a, b) {
      const ca = chunks(a), cb = chunks(b);
      for (let i = 0; i < Math.min(ca.length, cb.length); i++) {
        const x = ca[i], y = cb[i];
        if (/^\d/.test(x) && /^\d/.test(y)) {
          const nx = Number(x), ny = Number(y);
          if (nx !== ny) return nx < ny ? -1 : 1;
        } else {
          const c = primary(x, y);
          if (c !== 0) return c;
        }
      }
      return ca.length === cb.length ? 0 : ca.length < cb.length ? -1 : 1;
    }
  }
  // Root collation order in short: whitespace and punctuation, then digits, then letters.
  const klass = (ch) => (/\s/.test(ch) ? 0 : /[\d]/.test(ch) ? 2 : /\p{L}/u.test(ch) ? 3 : 1);
  const primary = (a, b) => {
    const A = Array.from(a), B = Array.from(b);
    for (let i = 0; i < Math.min(A.length, B.length); i++) {
      if (A[i] === B[i]) continue;
      const ka = klass(A[i]), kb = klass(B[i]);
      if (ka !== kb) return ka < kb ? -1 : 1;
      return A[i] < B[i] ? -1 : 1;
    }
    return A.length === B.length ? 0 : A.length < B.length ? -1 : 1;
  };

  // ---- the global and the built-in methods that use it -----------------------------------------------------------
  const Intl = {
    NumberFormat, DateTimeFormat, PluralRules, ListFormat, RelativeTimeFormat, Collator,
    getCanonicalLocales(l) { return (Array.isArray(l) ? l : l === undefined ? [] : [l]).map((x) => resolve(x).tag); },
    supportedValuesOf(key) {
      if (key === "currency") return Object.keys(DIGITS).concat(Object.keys(SYMBOLS)).filter((v, i, a) => a.indexOf(v) === i).sort();
      if (key === "unit") return Object.keys(UNITS).sort();
      throw rangeError("Invalid key : " + key);
    },
  };
  Object.defineProperty(Intl, Symbol.toStringTag, { value: "Intl" });
  Object.defineProperty(g, "Intl", { value: Intl, configurable: true, writable: true });
  const define = (proto, name, fn) => Object.defineProperty(proto, name, { value: fn, configurable: true, writable: true });
  define(Number.prototype, "toLocaleString", function (locales, options) { return new NumberFormat(locales, options).format(Number(this)); });
  if (typeof BigInt !== "undefined") define(BigInt.prototype, "toLocaleString", function (locales, options) { return new NumberFormat(locales, options).format(this); });
  define(String.prototype, "localeCompare", function (that, locales, options) { return new Collator(locales, options).compare(String(this), String(that)); });
  const dateWith = (defaults) => function (locales, options) {
    const o = Object.assign({}, options);
    const has = (keys) => keys.some((k) => o[k] !== undefined);
    if (!o.dateStyle && !o.timeStyle) {
      if (defaults.date && !has(["weekday", "year", "month", "day"]) && !(defaults.time && has(["hour", "minute", "second"]))) { if (!defaults.keepTime || !has(["hour", "minute", "second"])) { o.year = "numeric"; o.month = "numeric"; o.day = "numeric"; } }
      if (defaults.time && !has(["hour", "minute", "second"])) { o.hour = "numeric"; o.minute = "2-digit"; o.second = "2-digit"; }
    }
    return new DateTimeFormat(locales, o).format(this);
  };
  define(Date.prototype, "toLocaleDateString", dateWith({ date: true }));
  define(Date.prototype, "toLocaleTimeString", dateWith({ time: true }));
  define(Date.prototype, "toLocaleString", dateWith({ date: true, time: true }));
  // The OS language (and country) is the default locale.
  try {
    const lang = call("language"), country = call("country");
    if (lang && /^[a-z]{2,3}$/i.test(lang)) {
      const tag = lang.toLowerCase() + (country && /^[A-Za-z]{2}$/.test(country) ? "-" + country.toUpperCase() : "");
      Object.defineProperty(g.navigator, "language", { value: tag, configurable: true });
      Object.defineProperty(g.navigator, "languages", { value: [tag, lang.toLowerCase()], configurable: true });
    }
  } catch (e) { /* keep en-US */ }
}
