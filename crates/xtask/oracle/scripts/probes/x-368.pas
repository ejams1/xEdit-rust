{ parity probe: x-368 }
unit P_x_368;

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
  v := TList.Create; v.Add(5); v.Add(5); AddMessage(IntToStr(v.IndexOf(5)));
  AddMessage('after');
end;
end.
