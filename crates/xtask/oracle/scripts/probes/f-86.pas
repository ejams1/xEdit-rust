{ parity probe: f-86 }
unit P_f_86;

function Initialize: integer;
var
  lst: TList;
  sl: TStringList;
  s: string;
  i64: Int64;
  c: Cardinal;
  d: Double;
begin
  lst := TList.Create;
  sl := TStringList.Create;
  sl.Add('a');
  i64 := 5;
  c := 5;
  d := 1.5;
  s := Format('%s', [GetElementNativeValues(FileByIndex(0), 'File Header\HEDR\Number of Records')]);
  AddMessage('[' + s + ']');
end;
end.
