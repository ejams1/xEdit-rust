{ parity probe: x-261 }
unit P_x_261;

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
  v := IntToStr(1e10); AddMessage(v);
  AddMessage('after');
end;
end.
