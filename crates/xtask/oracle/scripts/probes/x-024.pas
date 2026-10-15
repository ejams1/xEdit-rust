{ parity probe: x-024 }
unit P_x_024;

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
  s := IntToStr(1, 2);
  AddMessage('after');
end;
end.
