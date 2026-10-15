{ parity probe: x-274 }
unit P_x_274;

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
  v := UpperCase(nil); AddMessage('[' + v + ']');
  AddMessage('after');
end;
end.
