{ parity probe: x-255 }
unit P_x_255;

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
  v := IntToStr(2.5); AddMessage(v);
  AddMessage('after');
end;
end.
