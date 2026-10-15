{ parity probe: f-83 }
unit P_f_83;

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
  s := Format('%s', [GetElementEditValues(FileByIndex(0), 'File Header\HEDR\Version')]);
  AddMessage('[' + s + ']');
end;
end.
