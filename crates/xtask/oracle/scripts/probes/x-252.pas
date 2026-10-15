{ parity probe: x-252 }
unit P_x_252;

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
  v := IntToStr('x');
  AddMessage('after');
end;
end.
