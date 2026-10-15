{ parity probe: x-250 }
unit P_x_250;

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
  v := IntToStr(sl);
  AddMessage('after');
end;
end.
