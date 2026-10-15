{ parity probe: x-256 }
unit P_x_256;

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
  v := IntToStr(True); AddMessage(v);
  AddMessage('after');
end;
end.
