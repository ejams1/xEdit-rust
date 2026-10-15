{ parity probe: x-262 }
unit P_x_262;

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
  v := IntToStr(1e19);
  AddMessage('after');
end;
end.
