{ parity probe: x-375 }
unit P_x_375;

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
  v := TList.Create; v.Add(FileByIndex(0)); AddMessage(IntToStr(v.IndexOf(nil)));
  AddMessage('after');
end;
end.
