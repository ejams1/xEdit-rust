{ parity probe: x-058 }
unit P_x_058;

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
  v := TStringList(sl); AddMessage(IntToStr(v.Count));
  AddMessage('after');
end;
end.
