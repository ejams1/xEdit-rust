{ parity probe: x-254 }
unit P_x_254;

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
  v := IntToStr(1.5); AddMessage(v);
  AddMessage('after');
end;
end.
