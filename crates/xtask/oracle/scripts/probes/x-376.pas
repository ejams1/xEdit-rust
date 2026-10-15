{ parity probe: x-376 }
unit P_x_376;

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
  v := TList.Create; v.Add(5); AddMessage(IntToStr(v.IndexOf(nil)));
  AddMessage('after');
end;
end.
