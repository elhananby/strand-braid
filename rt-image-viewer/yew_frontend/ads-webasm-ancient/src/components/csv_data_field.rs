use yew::prelude::*;
use yew::services::reader::{File, FileData, ReaderService, ReaderTask};

#[derive(PartialEq, Clone)]
pub struct CsvData<RowType> {
    filename: String,
    rows: Vec<RowType>,
    raw_buf: Vec<u8>,
}

impl<RowType> CsvData<RowType> {
    pub fn filename(&self) -> &str {
        &self.filename
    }
    pub fn rows(&self) -> &[RowType] {
        &self.rows
    }
    pub fn raw_buf(&self) -> &[u8] {
        &self.raw_buf
    }
}

pub enum MaybeCsvData<RowType> {
    Valid(CsvData<RowType>),
    Empty,
    ParseFail(String),
}

impl<RowType> std::fmt::Display for MaybeCsvData<RowType> {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        use self::MaybeCsvData::*;

        match &self {
            &Valid(ref fd) => write!(
                f,
                "CSV file \"{}\" with {} rows.",
                fd.filename,
                fd.rows.len()
            ),
            &Empty => write!(f, "No CSV file loaded."),
            &ParseFail(ref _e) => write!(f, "Failed parsing CSV file: {}", _e),
        }
    }
}

fn parse_csv<RowType>(filename: String, buf: &[u8]) -> MaybeCsvData<RowType>
where
    for<'de> RowType: serde::Deserialize<'de>,
{
    let raw_buf = buf.to_vec(); // copy raw data
    let rdr = csv::ReaderBuilder::new()
        .comment(Some(b'#'))
        .from_reader(buf);
    let mut rows = Vec::new();
    for row in rdr.into_deserialize().into_iter() {
        let row: RowType = match row {
            Ok(r) => r,
            Err(e) => return MaybeCsvData::ParseFail(format!("{}", e)),
        };
        rows.push(row);
    }
    MaybeCsvData::Valid(CsvData {
        filename,
        rows,
        raw_buf,
    })
}

impl<RowType> From<Option<CsvData<RowType>>> for MaybeCsvData<RowType> {
    fn from(orig: Option<CsvData<RowType>>) -> MaybeCsvData<RowType> {
        match orig {
            Some(val) => MaybeCsvData::Valid(val),
            None => MaybeCsvData::Empty,
        }
    }
}

pub struct CsvDataField<RowType>
where
    RowType: 'static + Clone + PartialEq,
    for<'de> RowType: serde::Deserialize<'de>,
{
    link: ComponentLink<Self>,
    reader: ReaderService,
    tasks: Vec<ReaderTask>,
    onfile: Option<Callback<MaybeCsvData<RowType>>>,
}

pub enum Msg {
    Loaded(FileData),
    Files(Vec<File>),
}

#[derive(PartialEq, Clone)]
pub struct Props<RowType> {
    pub onfile: Option<Callback<MaybeCsvData<RowType>>>,
}

impl<RowType> Default for Props<RowType> {
    fn default() -> Self {
        Props { onfile: None }
    }
}

impl<RowType> Component for CsvDataField<RowType>
where
    RowType: 'static + Clone + PartialEq,
    for<'de> RowType: serde::Deserialize<'de>,
{
    type Message = Msg;
    type Properties = Props<RowType>;

    fn create(props: Self::Properties, link: ComponentLink<Self>) -> Self {
        Self {
            link,
            reader: ReaderService::new(),
            tasks: vec![],
            onfile: props.onfile,
        }
    }

    fn update(&mut self, msg: Self::Message) -> ShouldRender {
        match msg {
            Msg::Loaded(file) => {
                let file = parse_csv(file.name, &file.content);
                if let Some(ref mut callback) = self.onfile {
                    callback.emit(file);
                }
            }
            Msg::Files(files) => {
                for file in files.into_iter() {
                    let callback = self.link.send_back(Msg::Loaded);
                    let task = self.reader.read_file(file, callback);
                    self.tasks.push(task);
                }
            }
        }
        true
    }

    fn change(&mut self, props: Self::Properties) -> ShouldRender {
        // self.parsed_local = props.current.into();
        self.onfile = props.onfile;
        true
    }
}

impl<RowType> Renderable<CsvDataField<RowType>> for CsvDataField<RowType>
where
    RowType: 'static + Clone + PartialEq,
    for<'de> RowType: serde::Deserialize<'de>,
{
    fn view(&self) -> Html<Self> {
        html! {
            <input type="file",
                class="custom-file-upload-input",
                multiple=false,
                accept=".csv",
                onchange=|value| {
                    let mut result = Vec::new();
                    if let ChangeData::Files(files) = value {
                        result.extend(files);
                    }
                    Msg::Files(result)
                },
                />
        }
    }
}
