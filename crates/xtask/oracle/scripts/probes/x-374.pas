{ parity probe: x-374 }
unit P_x_374;

function Initialize: integer;
var
  i: Integer;
  d: Double;
  s: string;
  b: Boolean;
  sl: TStringList;
  v: Variant;
begin
  sl := TStringList.Create;
  sl.Add('a');
  v := TList.Create; v.Add(FileByIndex(0)); AddMessage(IntToStr(v.IndexOf(FileByIndex(0))));
  AddMessage('after');
end;
end.
