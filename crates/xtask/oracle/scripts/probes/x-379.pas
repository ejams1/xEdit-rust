{ parity probe: x-379 }
unit P_x_379;

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
  v := TList.Create; v.Add(0); AddMessage(IntToStr(v.IndexOf(nil)));
  AddMessage('after');
end;
end.
